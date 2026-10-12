//! Network input handling for multiplayer games
//!
//! This module provides types and traits for handling player input in networked games.
//! The key feature is the [`NetworkInput`] derive macro which enforces required fields
//! at compile time, preventing common bugs like forgetting to set important fields.
//!
//! # Problem
//!
//! A common bug in multiplayer games is forgetting to populate all fields when
//! building input structs:
//!
//! ```ignore
//! // Bug: forgot to set facing_right!
//! let mut input = PlayerInput::default();
//! input.move_x = player.move_x;
//! input.jump = keys.jump;
//! // input.facing_right is still default (false)...
//! ```
//!
//! # Solution
//!
//! Use the `NetworkInput` derive macro with `#[network_input(required)]`:
//!
//! ```ignore
//! #[derive(NetworkInput, Clone, Default, Serialize, Deserialize)]
//! pub struct PlayerInput {
//!     #[network_input(required)]
//!     pub move_x: f32,
//!     #[network_input(required)]
//!     pub facing_right: bool,
//!     pub jump: bool,  // Optional
//! }
//!
//! // Compiler enforces required fields
//! let input = PlayerInput::builder()
//!     .move_x(1.0)
//!     .facing_right(true)
//!     .jump(keys.jump)
//!     .build();
//!
//! // This won't compile - missing facing_right:
//! // let input = PlayerInput::builder()
//! //     .move_x(1.0)
//! //     .build();  // ERROR!
//! ```
//!
//! # Usage
//!
//! 1. Add `#[derive(NetworkInput)]` to your input struct
//! 2. Mark critical fields with `#[network_input(required)]`
//! 3. Use the generated builder: `MyInput::builder().field(value).build()`
//!
//! # When to Use Required Fields
//!
//! Mark a field as required when:
//! - It affects gameplay in non-obvious ways (direction, targeting)
//! - A wrong default would cause bugs that are hard to notice
//! - It MUST be synchronized for correct gameplay
//!
//! Leave fields optional when:
//! - The default value is safe (false for buttons, 0 for magnitude)
//! - Missing the field just means "no action"

// Re-export the derive macro
pub use ryo2d_derive::NetworkInput;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Trait for types that can be used as network input.
///
/// This trait is automatically implemented by the `NetworkInput` derive macro.
/// It provides a common interface for input types used in multiplayer games.
///
/// # Example
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// #[derive(NetworkInput, Clone, Default, Serialize, Deserialize)]
/// pub struct MyInput {
///     #[network_input(required)]
///     pub direction: f32,
///     pub action: bool,
/// }
///
/// fn send_input<T: NetworkInputTrait>(input: &T) {
///     let bytes = serde_json::to_vec(input).unwrap();
///     // ... send over network
/// }
/// ```
pub trait NetworkInputTrait:
  Serialize + DeserializeOwned + Clone + Send + Sync + Default + 'static
{
  /// Get the name of this input type for debugging.
  fn input_name() -> &'static str;
}

/// Input with sequence number for client-side prediction reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedInput<T> {
  /// The input data
  pub input: T,
  /// Sequence number for this input (monotonically increasing per client)
  pub sequence: u32,
  /// Timestamp when this input was created (client time)
  pub timestamp: f64,
}

impl<T: Clone> SequencedInput<T> {
  /// Create a new sequenced input.
  pub fn new(input: T, sequence: u32, timestamp: f64) -> Self {
    Self {
      input,
      sequence,
      timestamp,
    }
  }
}

/// Ring buffer for storing pending inputs for reconciliation.
///
/// Used by clients to store inputs that have been sent but not yet
/// acknowledged by the server.
#[derive(Debug)]
pub struct InputBuffer<T> {
  buffer: Vec<Option<SequencedInput<T>>>,
  capacity: usize,
  last_acked_sequence: u32,
}

impl<T: Clone> InputBuffer<T> {
  /// Create a new input buffer with the specified capacity.
  ///
  /// Capacity should be large enough to hold inputs for at least
  /// one round-trip time's worth of frames (e.g., 60 for 1 second at 60fps).
  pub fn new(capacity: usize) -> Self {
    Self {
      buffer: vec![None; capacity],
      capacity,
      last_acked_sequence: 0,
    }
  }

  /// Store an input in the buffer.
  pub fn push(&mut self, input: SequencedInput<T>) {
    let index = (input.sequence as usize) % self.capacity;
    self.buffer[index] = Some(input);
  }

  /// Get an input by sequence number.
  pub fn get(&self, sequence: u32) -> Option<&SequencedInput<T>> {
    let index = (sequence as usize) % self.capacity;
    self.buffer[index]
      .as_ref()
      .filter(|i| i.sequence == sequence)
  }

  /// Mark inputs up to and including this sequence as acknowledged.
  ///
  /// Returns the number of inputs that were cleared.
  pub fn acknowledge(&mut self, sequence: u32) -> usize {
    let mut cleared = 0;
    for seq in (self.last_acked_sequence + 1)..=sequence {
      let index = (seq as usize) % self.capacity;
      if self.buffer[index]
        .as_ref()
        .is_some_and(|i| i.sequence == seq)
      {
        self.buffer[index] = None;
        cleared += 1;
      }
    }
    self.last_acked_sequence = sequence;
    cleared
  }

  /// Get all unacknowledged inputs after a given sequence.
  ///
  /// Used for replaying inputs after a server correction.
  pub fn inputs_after(&self, sequence: u32) -> impl Iterator<Item = &SequencedInput<T>> {
    self
      .buffer
      .iter()
      .filter_map(move |slot| slot.as_ref().filter(|i| i.sequence > sequence))
  }

  /// Clear all inputs.
  pub fn clear(&mut self) {
    self.buffer.fill(None);
  }

  /// Get the last acknowledged sequence number.
  pub fn last_acked(&self) -> u32 {
    self.last_acked_sequence
  }

  /// Get the next sequence number to use.
  pub fn next_sequence(&self) -> u32 {
    self.last_acked_sequence + 1 + self.pending_count() as u32
  }

  /// Count pending (unacknowledged) inputs.
  pub fn pending_count(&self) -> usize {
    self.buffer.iter().filter(|s| s.is_some()).count()
  }
}

// ============================================================================
// NetworkedInputHandler trait
// ============================================================================

use super::Channel;
use crate::core::ecs::Entity;
use crate::core::ecs::World;
use crate::core::input::Input;

/// Trait for handling networked player input with client-side prediction.
///
/// Implement this trait to define how input is gathered, applied, and reconciled
/// for a player entity. The trait enforces a consistent pattern:
///
/// 1. **Gather**: Collect input from local sources
/// 2. **Apply**: Apply input to the entity (same code on client and server)
/// 3. **Send**: Client sends to server
/// 4. **Reconcile**: Client replays inputs after server correction
///
/// # Example
///
/// ```ignore
/// struct LeapInputHandler {
///     controlled_entity: Option<Entity>,
/// }
///
/// impl NetworkedInputHandler for LeapInputHandler {
///     type Input = LeapInput;
///
///     fn gather_input(&self, state: &Input, _dt: f32) -> Self::Input {
///         LeapInput::builder()
///             .charging(state.key_held(Key::Space))
///             .facing_right(self.last_facing_right)
///             .aim_angle(self.aim_angle)
///             .build()
///     }
///
///     fn apply_input(&self, world: &mut World, entity: Entity, input: &Self::Input, dt: f32) {
///         if let Some(mut player) = world.get_mut::<Player>(entity) {
///             player.charging = input.charging;
///             player.facing_right = input.facing_right;
///             // Apply movement...
///         }
///     }
///
///     fn controlled_entity(&self) -> Option<Entity> {
///         self.controlled_entity
///     }
/// }
/// ```
pub trait NetworkedInputHandler {
  /// The input type used by this handler.
  type Input: Clone + Serialize + DeserializeOwned + Default;

  /// Gather input from local input sources.
  ///
  /// Called each frame on the client to build the input struct.
  fn gather_input(&self, state: &Input, dt: f32) -> Self::Input;

  /// Apply input to an entity.
  ///
  /// This is called on both client (for prediction) and server (for authoritative simulation).
  /// The same code should produce the same results given the same state.
  fn apply_input(&self, world: &mut World, entity: Entity, input: &Self::Input, dt: f32);

  /// Get the entity controlled by the local player.
  ///
  /// Returns `None` if no entity is being controlled (e.g., in spectator mode).
  fn controlled_entity(&self) -> Option<Entity>;

  /// Whether to apply input locally before server confirmation.
  ///
  /// Default is `true` for responsive gameplay. Override to `false` for
  /// turn-based games or actions that need server validation first.
  fn predict_locally(&self) -> bool {
    true
  }

  /// The channel to use for sending input.
  ///
  /// Default is `ReliableOrdered`. Override to `Unreliable` for high-frequency
  /// input where occasional loss is acceptable.
  fn channel(&self) -> Channel {
    Channel::ReliableOrdered
  }
}

/// Processes input according to the client-side prediction pattern.
///
/// This struct manages the full input lifecycle:
/// - Gathering input each frame
/// - Applying input locally (prediction)
/// - Sending input to server
/// - Storing inputs for reconciliation
/// - Replaying inputs after server correction
pub struct InputProcessor<H: NetworkedInputHandler> {
  handler: H,
  buffer: InputBuffer<H::Input>,
  sequence: u32,
  timestamp: f64,
}

impl<H: NetworkedInputHandler> InputProcessor<H> {
  /// Create a new input processor with the given handler.
  ///
  /// The buffer capacity should be large enough to hold inputs for at least
  /// one network round-trip time (e.g., 64 for ~1 second at 60fps).
  pub fn new(handler: H, buffer_capacity: usize) -> Self {
    Self {
      handler,
      buffer: InputBuffer::new(buffer_capacity),
      sequence: 0,
      timestamp: 0.0,
    }
  }

  /// Process a client frame: gather input, apply locally, store for reconciliation.
  ///
  /// Returns the sequenced input that should be sent to the server.
  pub fn process_client_frame(
    &mut self,
    world: &mut World,
    input_state: &Input,
    dt: f32,
    current_time: f64,
  ) -> Option<SequencedInput<H::Input>> {
    let entity = self.handler.controlled_entity()?;

    // Gather input
    let input = self.handler.gather_input(input_state, dt);

    // Increment sequence
    self.sequence += 1;
    self.timestamp = current_time;

    // Create sequenced input
    let sequenced = SequencedInput::new(input.clone(), self.sequence, current_time);

    // Store for reconciliation
    self.buffer.push(sequenced.clone());

    // Apply locally if predicting
    if self.handler.predict_locally() {
      self.handler.apply_input(world, entity, &input, dt);
    }

    Some(sequenced)
  }

  /// Reconcile with server state after receiving a correction.
  ///
  /// This replays all unacknowledged inputs on top of the server's state.
  ///
  /// # Arguments
  ///
  /// * `world` - The world with the corrected server state applied
  /// * `server_sequence` - The sequence number the server has processed up to
  /// * `dt` - The time step to use for replaying each input
  pub fn reconcile(&mut self, world: &mut World, server_sequence: u32, dt: f32) {
    // Mark inputs up to server_sequence as acknowledged
    self.buffer.acknowledge(server_sequence);

    // Get the controlled entity
    let Some(entity) = self.handler.controlled_entity() else {
      return;
    };

    // Collect inputs to replay (need to sort by sequence)
    let mut inputs: Vec<_> = self.buffer.inputs_after(server_sequence).collect();
    inputs.sort_by_key(|i| i.sequence);

    // Replay unacknowledged inputs
    for sequenced in inputs {
      self
        .handler
        .apply_input(world, entity, &sequenced.input, dt);
    }
  }

  /// Get a reference to the underlying handler.
  pub fn handler(&self) -> &H {
    &self.handler
  }

  /// Get a mutable reference to the underlying handler.
  pub fn handler_mut(&mut self) -> &mut H {
    &mut self.handler
  }

  /// Get the current sequence number.
  pub fn current_sequence(&self) -> u32 {
    self.sequence
  }

  /// Get the number of pending (unacknowledged) inputs.
  pub fn pending_count(&self) -> usize {
    self.buffer.pending_count()
  }

  /// Clear all pending inputs (e.g., after respawn).
  pub fn clear_pending(&mut self) {
    self.buffer.clear();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::ecs::Transform;

  // Test input handler for unit tests
  struct TestHandler {
    entity: Option<Entity>,
    predict: bool,
  }

  impl NetworkedInputHandler for TestHandler {
    type Input = i32;

    fn gather_input(&self, _state: &Input, _dt: f32) -> Self::Input {
      42
    }

    fn apply_input(&self, world: &mut World, entity: Entity, input: &Self::Input, _dt: f32) {
      if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation.x += *input as f32;
      }
    }

    fn controlled_entity(&self) -> Option<Entity> {
      self.entity
    }

    fn predict_locally(&self) -> bool {
      self.predict
    }
  }

  #[test]
  fn input_processor_basic() {
    let mut world = World::new();
    let entity = world.spawn((Transform::default(),)).id();

    let handler = TestHandler {
      entity: Some(entity),
      predict: true,
    };
    let mut processor = InputProcessor::new(handler, 64);
    let input_state = Input::default();

    // Process a frame
    let result = processor.process_client_frame(&mut world, &input_state, 0.016, 0.0);
    assert!(result.is_some());

    let sequenced = result.unwrap();
    assert_eq!(sequenced.sequence, 1);
    assert_eq!(sequenced.input, 42);

    // Check that input was applied (prediction)
    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 42.0);
  }

  #[test]
  fn input_processor_no_prediction() {
    let mut world = World::new();
    let entity = world.spawn((Transform::default(),)).id();

    let handler = TestHandler {
      entity: Some(entity),
      predict: false,
    };
    let mut processor = InputProcessor::new(handler, 64);
    let input_state = Input::default();

    // Process a frame
    let _ = processor.process_client_frame(&mut world, &input_state, 0.016, 0.0);

    // Input should NOT be applied locally
    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 0.0);
  }

  #[test]
  fn input_processor_reconcile() {
    let mut world = World::new();
    let entity = world.spawn((Transform::default(),)).id();

    let handler = TestHandler {
      entity: Some(entity),
      predict: true,
    };
    let mut processor = InputProcessor::new(handler, 64);
    let input_state = Input::default();

    // Process several frames
    for _ in 0..3 {
      processor.process_client_frame(&mut world, &input_state, 0.016, 0.0);
    }

    // Simulate server correction: reset position and acknowledge seq 1
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
      transform.translation.x = 100.0; // Server says we're at 100
    }

    // Reconcile: replay inputs 2 and 3
    processor.reconcile(&mut world, 1, 0.016);

    // Should be 100 + 42 + 42 = 184
    let transform = world.get::<Transform>(entity).unwrap();
    assert_eq!(transform.translation.x, 184.0);
  }

  #[test]
  fn input_processor_no_entity() {
    let mut world = World::new();

    let handler = TestHandler {
      entity: None,
      predict: true,
    };
    let mut processor = InputProcessor::new(handler, 64);
    let input_state = Input::default();

    // Process should return None when no entity
    let result = processor.process_client_frame(&mut world, &input_state, 0.016, 0.0);
    assert!(result.is_none());
  }

  #[test]
  fn input_buffer_basic() {
    let mut buffer: InputBuffer<i32> = InputBuffer::new(64);

    buffer.push(SequencedInput::new(100, 1, 0.0));
    buffer.push(SequencedInput::new(200, 2, 0.0));
    buffer.push(SequencedInput::new(300, 3, 0.0));

    assert_eq!(buffer.get(1).map(|i| i.input), Some(100));
    assert_eq!(buffer.get(2).map(|i| i.input), Some(200));
    assert_eq!(buffer.get(3).map(|i| i.input), Some(300));
    assert!(buffer.get(4).is_none());
  }

  #[test]
  fn input_buffer_acknowledge() {
    let mut buffer: InputBuffer<i32> = InputBuffer::new(64);

    buffer.push(SequencedInput::new(100, 1, 0.0));
    buffer.push(SequencedInput::new(200, 2, 0.0));
    buffer.push(SequencedInput::new(300, 3, 0.0));

    let cleared = buffer.acknowledge(2);
    assert_eq!(cleared, 2);
    assert!(buffer.get(1).is_none());
    assert!(buffer.get(2).is_none());
    assert!(buffer.get(3).is_some());
  }

  #[test]
  fn input_buffer_inputs_after() {
    let mut buffer: InputBuffer<i32> = InputBuffer::new(64);

    buffer.push(SequencedInput::new(100, 1, 0.0));
    buffer.push(SequencedInput::new(200, 2, 0.0));
    buffer.push(SequencedInput::new(300, 3, 0.0));

    let after: Vec<_> = buffer.inputs_after(1).collect();
    assert_eq!(after.len(), 2);
  }

  #[test]
  fn input_buffer_wraparound() {
    let mut buffer: InputBuffer<i32> = InputBuffer::new(4);

    // Fill buffer
    for i in 0..4 {
      buffer.push(SequencedInput::new(i * 100, i as u32, 0.0));
    }

    // Wraparound
    buffer.push(SequencedInput::new(400, 4, 0.0));

    // Slot 0 should now have sequence 4
    assert_eq!(buffer.get(4).map(|i| i.input), Some(400));
    // Old sequence 0 should be gone
    assert!(buffer.get(0).is_none());
  }

  #[test]
  fn sequenced_input_creation() {
    let input = SequencedInput::new("hello", 42, 1.5);
    assert_eq!(input.input, "hello");
    assert_eq!(input.sequence, 42);
    assert_eq!(input.timestamp, 1.5);
  }
}
