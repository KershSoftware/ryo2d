//! NetworkAction trait for dual local+network execution
//!
//! This module provides the [`NetworkAction`] trait and helper functions for
//! actions that need to execute both locally AND be sent over the network.
//!
//! # Problem
//!
//! A common bug in multiplayer games is handling actions that need to happen
//! both locally (for responsiveness) and on the network (for synchronization):
//!
//! ```ignore
//! // Bug: easy to forget one half
//! fn respawn(&mut self) {
//!     self.send_respawn_to_server();
//!     // Forgot to respawn locally!
//! }
//!
//! // Or the opposite bug:
//! fn respawn(&mut self) {
//!     self.respawn_local_player();
//!     // Forgot to send to server!
//! }
//! ```
//!
//! # Solution
//!
//! Use [`NetworkAction`] to define actions that execute both locally and remotely:
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! #[derive(Clone, Serialize, Deserialize)]
//! struct RespawnAction {
//!     slot: u8,
//!     spawn_x: f32,
//!     spawn_y: f32,
//! }
//!
//! impl NetworkAction for RespawnAction {
//!     fn action_id() -> RpcTypeId {
//!         RpcTypeId::from_name("Respawn")
//!     }
//!
//!     fn action_name() -> &'static str {
//!         "Respawn"
//!     }
//!
//!     fn apply(&self, ctx: &mut ActionContext) {
//!         if let Some(entity) = ctx.get_player_entity(self.slot) {
//!             // Reset position
//!             if let Some(mut transform) = ctx.world.get_mut::<Transform>(entity) {
//!                 transform.x = self.spawn_x;
//!                 transform.y = self.spawn_y;
//!             }
//!             // Reset state
//!             // ...
//!         }
//!     }
//! }
//!
//! // Single call does both local execution AND network send
//! execute_action(RespawnAction { slot: 1, spawn_x: 0.0, spawn_y: 0.0 }, &mut ctx);
//! ```
//!
//! # Benefits
//!
//! - **Can't forget either half**: One call does both
//! - **Consistent state**: Same code runs on all machines
//! - **Easy to test**: Action logic is isolated in `apply()`
//! - **Predictable**: Local execution happens immediately

use renet::ClientId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::rpc::RpcTypeId;
use super::{Channel, NetworkClient, ServerLike};
use crate::core::ecs::World;

/// Trait for actions that execute both locally and are sent over the network.
///
/// Actions are serializable commands that modify game state. When executed,
/// they apply locally for immediate feedback and are sent over the network
/// for synchronization.
///
/// # Example
///
/// ```ignore
/// #[derive(Clone, Serialize, Deserialize)]
/// struct JumpAction {
///     player_id: ClientId,
///     force: f32,
/// }
///
/// impl NetworkAction for JumpAction {
///     fn action_id() -> RpcTypeId {
///         RpcTypeId::from_name("Jump")
///     }
///
///     fn action_name() -> &'static str { "Jump" }
///
///     fn apply(&self, ctx: &mut ActionContext) {
///         // Apply jump velocity to player
///     }
/// }
/// ```
pub trait NetworkAction: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
  /// Get the unique type ID for this action.
  fn action_id() -> RpcTypeId;

  /// Get the human-readable name of this action.
  fn action_name() -> &'static str;

  /// Apply the action to the game state.
  ///
  /// This is called both locally (immediately) and on remote machines
  /// (after receiving over the network).
  fn apply(&self, ctx: &mut ActionContext);

  /// Whether this action should be applied locally before sending.
  ///
  /// Override to return `false` for actions that should only be
  /// applied after server confirmation.
  fn apply_locally(&self) -> bool {
    true
  }

  /// The channel to use for sending this action.
  ///
  /// Override to use `Unreliable` for frequent, loss-tolerant actions.
  fn channel(&self) -> Channel {
    Channel::ReliableOrdered
  }
}

/// Context provided to [`NetworkAction::apply`].
///
/// Contains all the state an action needs to modify the game.
pub struct ActionContext<'a> {
  /// The ECS world
  pub world: &'a mut World,
  /// User data for game-specific lookups
  pub user_data: Option<&'a mut dyn std::any::Any>,
}

impl<'a> ActionContext<'a> {
  /// Create a new action context.
  pub fn new(world: &'a mut World) -> Self {
    Self {
      world,
      user_data: None,
    }
  }

  /// Create a new action context with user data.
  pub fn with_user_data(world: &'a mut World, user_data: &'a mut dyn std::any::Any) -> Self {
    Self {
      world,
      user_data: Some(user_data),
    }
  }

  /// Get the user data as a specific type.
  pub fn get_user_data<T: 'static>(&mut self) -> Option<&mut T> {
    self.user_data.as_mut()?.downcast_mut::<T>()
  }
}

/// Serialized network action for transmission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedAction {
  /// The action type identifier
  pub action_id: RpcTypeId,
  /// The action name (for debugging)
  pub name: String,
  /// Serialized payload
  pub payload: Vec<u8>,
  /// Who initiated this action
  pub sender: ClientId,
}

impl SerializedAction {
  /// Create a serialized action from a typed action.
  pub fn from_action<A: NetworkAction>(
    action: &A,
    sender: ClientId,
  ) -> Result<Self, serde_json::Error> {
    Ok(Self {
      action_id: A::action_id(),
      name: A::action_name().to_string(),
      payload: serde_json::to_vec(action)?,
      sender,
    })
  }

  /// Deserialize into a typed action.
  pub fn into_action<A: NetworkAction>(self) -> Result<A, serde_json::Error> {
    serde_json::from_slice(&self.payload)
  }

  /// Serialize to bytes for network transmission.
  pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(self)
  }

  /// Deserialize from bytes.
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
    serde_json::from_slice(bytes)
  }
}

/// Execute an action locally and send it to the server.
///
/// This is the main function for client-side action execution:
/// 1. Applies the action locally (if `apply_locally()` returns true)
/// 2. Serializes and sends to the server
///
/// # Example
///
/// ```ignore
/// // Client wants to jump
/// execute_action(JumpAction { force: 10.0 }, &mut ctx, &mut client);
/// ```
pub fn execute_action<A: NetworkAction>(
  action: A,
  ctx: &mut ActionContext,
  client: &mut NetworkClient,
) {
  // Apply locally first for immediate feedback
  if action.apply_locally() {
    action.apply(ctx);
  }

  // Send to server
  let client_id = client.client_id();
  if let Ok(serialized) = SerializedAction::from_action(&action, client_id)
    && let Ok(bytes) = serialized.to_bytes()
  {
    client.send(action.channel(), &bytes);
  }
}

/// Execute an action locally and send it to the server (returns error on failure).
///
/// Like [`execute_action`] but returns errors instead of silently failing.
pub fn try_execute_action<A: NetworkAction>(
  action: A,
  ctx: &mut ActionContext,
  client: &mut NetworkClient,
) -> Result<(), ActionError> {
  // Apply locally first
  if action.apply_locally() {
    action.apply(ctx);
  }

  // Send to server
  let client_id = client.client_id();
  let serialized = SerializedAction::from_action(&action, client_id)
    .map_err(|e| ActionError::Serialization(e.to_string()))?;
  let bytes = serialized
    .to_bytes()
    .map_err(|e| ActionError::Serialization(e.to_string()))?;
  client.send(action.channel(), &bytes);
  Ok(())
}

/// Server-side: execute an action and broadcast to other clients.
///
/// Use this when the server receives an action from a client:
/// 1. Applies the action locally (on server)
/// 2. Broadcasts to all other clients
///
/// # Example
///
/// ```ignore
/// // Server receives action from client
/// let action: JumpAction = serialized.into_action()?;
/// execute_action_server(action, &mut ctx, &mut server, sender_client_id);
/// ```
pub fn execute_action_server<A: NetworkAction, S: ServerLike>(
  action: A,
  ctx: &mut ActionContext,
  server: &mut S,
  sender: ClientId,
) {
  // Apply on server
  action.apply(ctx);

  // Broadcast to all clients except sender
  if let Ok(serialized) = SerializedAction::from_action(&action, sender)
    && let Ok(bytes) = serialized.to_bytes()
  {
    // Collect clients first to avoid borrow issues
    let clients: Vec<ClientId> = server.clients().collect();
    for client_id in clients {
      if client_id != sender && !server.is_host(client_id) {
        server.send(client_id, action.channel(), &bytes);
      }
    }
  }
}

/// Server-side: execute an action and broadcast to ALL clients (including sender).
///
/// Use this for server-initiated actions that all clients need to see.
pub fn execute_action_broadcast<A: NetworkAction, S: ServerLike>(
  action: A,
  ctx: &mut ActionContext,
  server: &mut S,
) {
  // Apply on server
  action.apply(ctx);

  // Broadcast to all remote clients
  if let Ok(serialized) = SerializedAction::from_action(&action, 0)
    && let Ok(bytes) = serialized.to_bytes()
  {
    server.broadcast(action.channel(), &bytes);
  }
}

/// Apply a received action (client receiving from server).
///
/// Use this when a client receives an action from the server.
/// The action is applied locally without re-sending.
pub fn apply_received_action<A: NetworkAction>(action: A, ctx: &mut ActionContext) {
  action.apply(ctx);
}

/// Errors that can occur during action execution.
#[derive(Debug, Clone)]
pub enum ActionError {
  /// Failed to serialize the action
  Serialization(String),
  /// Failed to deserialize the action
  Deserialization(String),
  /// Network send failed
  SendFailed(String),
  /// Unknown action type
  UnknownAction(RpcTypeId),
}

impl std::fmt::Display for ActionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ActionError::Serialization(e) => write!(f, "Action serialization failed: {}", e),
      ActionError::Deserialization(e) => write!(f, "Action deserialization failed: {}", e),
      ActionError::SendFailed(e) => write!(f, "Action send failed: {}", e),
      ActionError::UnknownAction(id) => write!(f, "Unknown action type: {}", id),
    }
  }
}

impl std::error::Error for ActionError {}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  struct TestAction {
    value: i32,
  }

  impl NetworkAction for TestAction {
    fn action_id() -> RpcTypeId {
      RpcTypeId::from_name("TestAction")
    }

    fn action_name() -> &'static str {
      "TestAction"
    }

    fn apply(&self, _ctx: &mut ActionContext) {
      // Test action does nothing
    }
  }

  #[test]
  fn serialized_action_roundtrip() {
    let action = TestAction { value: 42 };
    let serialized = SerializedAction::from_action(&action, 1).unwrap();

    assert_eq!(serialized.action_id, TestAction::action_id());
    assert_eq!(serialized.name, "TestAction");
    assert_eq!(serialized.sender, 1);

    let deserialized: TestAction = serialized.into_action().unwrap();
    assert_eq!(deserialized.value, 42);
  }

  #[test]
  fn serialized_action_bytes_roundtrip() {
    let action = TestAction { value: 123 };
    let serialized = SerializedAction::from_action(&action, 5).unwrap();
    let bytes = serialized.to_bytes().unwrap();
    let recovered = SerializedAction::from_bytes(&bytes).unwrap();

    assert_eq!(recovered.action_id, TestAction::action_id());
    assert_eq!(recovered.sender, 5);

    let deserialized: TestAction = recovered.into_action().unwrap();
    assert_eq!(deserialized.value, 123);
  }

  #[test]
  fn action_context_user_data() {
    let mut world = World::new();
    let mut data = String::from("hello");

    let mut ctx = ActionContext::with_user_data(&mut world, &mut data);
    if let Some(s) = ctx.get_user_data::<String>() {
      s.push_str(" world");
    }

    assert_eq!(data, "hello world");
  }

  #[test]
  fn action_default_apply_locally() {
    let action = TestAction { value: 0 };
    assert!(action.apply_locally());
  }

  #[test]
  fn action_default_channel() {
    let action = TestAction { value: 0 };
    assert_eq!(action.channel(), Channel::ReliableOrdered);
  }

  #[test]
  fn action_error_display() {
    let err = ActionError::Serialization("test".to_string());
    assert!(err.to_string().contains("serialization"));

    let err = ActionError::UnknownAction(RpcTypeId::from_name("Test"));
    assert!(err.to_string().contains("Unknown action"));
  }
}
