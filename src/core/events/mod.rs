//! Event Bus System
//!
//! A decoupled publish-subscribe system for game events. Events allow
//! different systems to communicate without direct dependencies.
//!
//! # Overview
//!
//! The event bus provides:
//! - **Decoupling**: Systems emit events without knowing who listens
//! - **Type safety**: Events are strongly-typed Rust structs
//! - **Frame-based lifetime**: Events live for one frame, then cleared
//! - **Multiple readers**: Any number of systems can read the same events
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use ryo2d::prelude::*;
//!
//! // Define your own event types
//! struct PlayerDied { entity: Entity }
//! struct ScoreChanged { delta: i32, new_total: u32 }
//!
//! // Emit events from game logic
//! fn update(&mut self, ctx: &mut GameContext) {
//!     if player_health <= 0 {
//!         ctx.events.emit(PlayerDied { entity: self.player });
//!     }
//! }
//!
//! // Read events from any system
//! fn update_audio(&mut self, ctx: &mut GameContext) {
//!     for _ in ctx.events.read::<PlayerDied>() {
//!         self.play_sound("death.wav");
//!     }
//! }
//! ```
//!
//! # Built-in Events
//!
//! The engine emits several built-in events:
//!
//! | Event | Source | Description |
//! |-------|--------|-------------|
//! | [`CollisionStarted`] | Physics | Two entities began colliding |
//! | [`CollisionStopped`] | Physics | Two entities stopped colliding |
//! | [`TweenStarted`] | Tweening | A tween began playing (after delay) |
//! | [`TweenCompleted`] | Tweening | A tween finished an iteration |
//! | [`ClientConnected`] | Network | A client joined (host only) |
//! | [`ClientDisconnected`] | Network | A client left (host only) |
//! | [`ConnectedToServer`] | Network | Connected to server (client only) |
//! | [`DisconnectedFromServer`] | Network | Lost server connection (client only) |
//! | [`AssetReloaded`] | Assets | An asset was hot-reloaded |
//! | [`AssetLoadFailed`] | Assets | An asset failed to load |
//!
//! # Event Lifetime
//!
//! Events exist only for the frame they are emitted:
//!
//! ```text
//! Frame N:
//!   emit(PlayerDied)     ← Event created
//!   read::<PlayerDied>() ← Returns the event
//!   read::<PlayerDied>() ← Still returns it (multiple reads OK)
//!
//! Frame N+1:
//!   clear()              ← Engine clears all events
//!   read::<PlayerDied>() ← Returns nothing
//! ```
//!
//! # Performance
//!
//! - Events are stored in contiguous `Vec`s per type
//! - Type lookup uses `TypeId` (fast hash)
//! - No allocations after warmup (vecs reuse capacity)
//! - Zero cost for event types that aren't used

mod builtin;
mod channel;

pub use builtin::*;

use channel::{AnyEventChannel, EventChannel};
use std::any::TypeId;
use std::collections::HashMap;

/// Marker trait for event types.
///
/// Any type that is `'static + Send + Sync` automatically implements this trait.
/// You don't need to implement it manually.
///
/// # Example
///
/// ```rust,ignore
/// // This automatically implements Event
/// struct MyEvent {
///     value: i32,
///     name: String,
/// }
/// ```
pub trait Event: 'static + Send + Sync {}

// Blanket implementation for all qualifying types
impl<T: 'static + Send + Sync> Event for T {}

/// The event bus - stores and distributes events between systems.
///
/// Events are emitted with [`emit`](Events::emit) and read with [`read`](Events::read).
/// All events are cleared at the start of each frame by the engine.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// struct EnemySpawned { entity: Entity, enemy_type: String }
///
/// fn spawn_enemy(&mut self, ctx: &mut GameContext) {
///     let entity = ctx.world.spawn((
///         Transform::new(100.0, 100.0),
///         Enemy { health: 100 },
///     ));
///
///     ctx.events.emit(EnemySpawned {
///         entity,
///         enemy_type: "goblin".into(),
///     });
/// }
///
/// fn update_ui(&mut self, ctx: &mut GameContext) {
///     for event in ctx.events.read::<EnemySpawned>() {
///         self.show_spawn_indicator(&event.enemy_type);
///     }
/// }
/// ```
#[derive(Default)]
pub struct Events {
  channels: HashMap<TypeId, Box<dyn AnyEventChannel>>,
}

impl Events {
  /// Create a new empty event bus.
  pub fn new() -> Self {
    Self {
      channels: HashMap::new(),
    }
  }

  /// Emit an event to be read by other systems this frame.
  ///
  /// Events are available immediately after emission and persist until
  /// the end of the frame when [`clear`](Events::clear) is called.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// ctx.events.emit(PlayerDied { entity: player });
  /// ctx.events.emit(ScoreChanged { delta: 100, new_total: 1500 });
  /// ```
  pub fn emit<E: Event>(&mut self, event: E) {
    let type_id = TypeId::of::<E>();

    let channel = self
      .channels
      .entry(type_id)
      .or_insert_with(|| Box::new(EventChannel::<E>::new()));

    // Safe because we just created/retrieved a channel of type E
    let typed_channel = channel
      .as_any_mut()
      .downcast_mut::<EventChannel<E>>()
      .expect("Event channel type mismatch");

    typed_channel.push(event);
  }

  /// Read all events of type `E` emitted this frame.
  ///
  /// Returns an iterator over references to the events. Multiple systems
  /// can read the same events - reading does not consume them.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Read all collision events
  /// for event in ctx.events.read::<CollisionStarted>() {
  ///     if event.involves(player) {
  ///         handle_player_collision(event);
  ///     }
  /// }
  ///
  /// // Check for specific events
  /// if ctx.events.has::<PlayerDied>() {
  ///     show_game_over();
  /// }
  /// ```
  pub fn read<E: Event>(&self) -> impl Iterator<Item = &E> {
    let type_id = TypeId::of::<E>();

    self
      .channels
      .get(&type_id)
      .and_then(|channel| channel.as_any().downcast_ref::<EventChannel<E>>())
      .map(|channel| channel.iter())
      .into_iter()
      .flatten()
  }

  /// Check if any events of type `E` have been emitted this frame.
  ///
  /// More efficient than `read().next().is_some()` when you only need
  /// to check existence.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// if ctx.events.has::<PlayerDied>() {
  ///     transition_to_game_over();
  /// }
  /// ```
  pub fn has<E: Event>(&self) -> bool {
    let type_id = TypeId::of::<E>();

    self
      .channels
      .get(&type_id)
      .map(|channel| !channel.is_empty())
      .unwrap_or(false)
  }

  /// Count the number of events of type `E` emitted this frame.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// let enemies_killed = ctx.events.count::<EnemyDied>();
  /// if enemies_killed >= 10 {
  ///     unlock_achievement("Mass Destruction");
  /// }
  /// ```
  pub fn count<E: Event>(&self) -> usize {
    let type_id = TypeId::of::<E>();

    self
      .channels
      .get(&type_id)
      .map(|channel| channel.len())
      .unwrap_or(0)
  }

  /// Clear all events from all channels.
  ///
  /// This is called automatically by the engine at the start of each frame.
  /// You typically don't need to call this manually.
  pub fn clear(&mut self) {
    for channel in self.channels.values_mut() {
      channel.clear();
    }
  }

  /// Get the total number of events across all types.
  ///
  /// Useful for debugging and profiling.
  pub fn total_event_count(&self) -> usize {
    self.channels.values().map(|c| c.len()).sum()
  }

  /// Get the number of different event types that have been used.
  ///
  /// Useful for debugging and profiling.
  pub fn channel_count(&self) -> usize {
    self.channels.len()
  }
}

impl std::fmt::Debug for Events {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("Events")
      .field("channels", &self.channels.len())
      .field("total_events", &self.total_event_count())
      .finish()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, Clone, PartialEq)]
  struct TestEvent {
    value: i32,
  }

  #[derive(Debug, Clone, PartialEq)]
  struct OtherEvent {
    name: String,
  }

  #[test]
  fn emit_and_read_single_event() {
    let mut events = Events::new();

    events.emit(TestEvent { value: 42 });

    let collected: Vec<_> = events.read::<TestEvent>().collect();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].value, 42);
  }

  #[test]
  fn emit_and_read_multiple_events() {
    let mut events = Events::new();

    events.emit(TestEvent { value: 1 });
    events.emit(TestEvent { value: 2 });
    events.emit(TestEvent { value: 3 });

    let collected: Vec<_> = events.read::<TestEvent>().collect();
    assert_eq!(collected.len(), 3);
    assert_eq!(collected[0].value, 1);
    assert_eq!(collected[1].value, 2);
    assert_eq!(collected[2].value, 3);
  }

  #[test]
  fn different_event_types_are_separate() {
    let mut events = Events::new();

    events.emit(TestEvent { value: 42 });
    events.emit(OtherEvent {
      name: "hello".into(),
    });

    assert_eq!(events.read::<TestEvent>().count(), 1);
    assert_eq!(events.read::<OtherEvent>().count(), 1);
  }

  #[test]
  fn read_nonexistent_event_type_returns_empty() {
    let events = Events::new();

    let collected: Vec<_> = events.read::<TestEvent>().collect();
    assert!(collected.is_empty());
  }

  #[test]
  fn has_returns_correct_value() {
    let mut events = Events::new();

    assert!(!events.has::<TestEvent>());

    events.emit(TestEvent { value: 1 });

    assert!(events.has::<TestEvent>());
    assert!(!events.has::<OtherEvent>());
  }

  #[test]
  fn count_returns_correct_value() {
    let mut events = Events::new();

    assert_eq!(events.count::<TestEvent>(), 0);

    events.emit(TestEvent { value: 1 });
    events.emit(TestEvent { value: 2 });

    assert_eq!(events.count::<TestEvent>(), 2);
    assert_eq!(events.count::<OtherEvent>(), 0);
  }

  #[test]
  fn clear_removes_all_events() {
    let mut events = Events::new();

    events.emit(TestEvent { value: 1 });
    events.emit(OtherEvent {
      name: "test".into(),
    });

    assert!(events.has::<TestEvent>());
    assert!(events.has::<OtherEvent>());

    events.clear();

    assert!(!events.has::<TestEvent>());
    assert!(!events.has::<OtherEvent>());
  }

  #[test]
  fn multiple_reads_return_same_events() {
    let mut events = Events::new();

    events.emit(TestEvent { value: 42 });

    // First read
    let first: Vec<_> = events.read::<TestEvent>().collect();
    // Second read
    let second: Vec<_> = events.read::<TestEvent>().collect();

    assert_eq!(first, second);
  }

  #[test]
  fn collision_started_involves_helper() {
    // Create real entities using a world
    let mut world = hecs::World::new();
    let entity_a = world.spawn(());
    let entity_b = world.spawn(());
    let entity_c = world.spawn(());

    let event = CollisionStarted { entity_a, entity_b };

    assert!(event.involves(entity_a));
    assert!(event.involves(entity_b));
    assert!(!event.involves(entity_c));
  }

  #[test]
  fn collision_started_other_helper() {
    // Create real entities using a world
    let mut world = hecs::World::new();
    let entity_a = world.spawn(());
    let entity_b = world.spawn(());
    let entity_c = world.spawn(());

    let event = CollisionStarted { entity_a, entity_b };

    assert_eq!(event.other(entity_a), Some(entity_b));
    assert_eq!(event.other(entity_b), Some(entity_a));
    assert_eq!(event.other(entity_c), None);
  }
}
