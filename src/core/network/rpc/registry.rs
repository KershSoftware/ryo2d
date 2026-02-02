//! RPC registry for handler registration and dispatch
//!
//! The registry maintains RPC handlers and dispatches incoming RPCs
//! to the appropriate handler functions.

use std::any::TypeId;
use std::collections::HashMap;

use renet::ClientId;

use super::types::{Rpc, RpcTypeId, SerializedRpc};
use crate::core::ecs::World;
use crate::core::error::ReplicationError;

/// Type-erased RPC handler function.
type RpcHandler =
  Box<dyn Fn(&mut World, &[u8], ClientId) -> Result<(), ReplicationError> + Send + Sync>;

/// Registry for RPC handlers.
///
/// The registry maintains handlers for different RPC types and dispatches
/// incoming RPCs to the appropriate handler.
///
/// # Example
///
/// ```ignore
/// // In your server code:
/// let mut registry = RpcRegistry::new();
///
/// // Register a handler (PlayerInputRpc defined elsewhere)
/// registry.register::<PlayerInputRpc>(|world, rpc| {
///     // Handle the RPC
///     println!("Received input from {}: {:?}", rpc.sender, rpc.payload);
///     Ok(())
/// });
///
/// // Dispatch incoming RPCs
/// registry.dispatch(&mut world, &serialized_rpc)?;
/// ```
pub struct RpcRegistry {
  /// Maps RpcTypeId to handler function
  handlers: HashMap<RpcTypeId, RpcHandler>,

  /// Maps TypeId to RpcTypeId for type lookup
  type_to_id: HashMap<TypeId, RpcTypeId>,

  /// Maps RpcTypeId to name for debugging
  id_to_name: HashMap<RpcTypeId, &'static str>,
}

impl RpcRegistry {
  /// Create a new empty RPC registry.
  pub fn new() -> Self {
    Self {
      handlers: HashMap::new(),
      type_to_id: HashMap::new(),
      id_to_name: HashMap::new(),
    }
  }

  /// Register an RPC handler.
  ///
  /// The handler receives the world, the deserialized RPC, and the sender's client ID.
  ///
  /// # Type Parameters
  ///
  /// * `T` - The RPC type to handle
  /// * `F` - The handler function
  pub fn register<T, F>(&mut self, handler: F)
  where
    T: Rpc,
    F: Fn(&mut World, &super::types::ReceivedRpc<T>) -> Result<(), ReplicationError>
      + Send
      + Sync
      + 'static,
  {
    let rpc_id = T::rpc_id();
    let type_id = TypeId::of::<T>();

    self.type_to_id.insert(type_id, rpc_id);
    self.id_to_name.insert(rpc_id, T::rpc_name());

    // Create type-erased handler
    self.handlers.insert(
      rpc_id,
      Box::new(move |world, payload, sender| {
        let rpc: T = serde_json::from_slice(payload).map_err(|e| {
          ReplicationError::RpcError(format!("Failed to deserialize {}: {}", T::rpc_name(), e))
        })?;

        let received = super::types::ReceivedRpc::new(rpc, sender);
        handler(world, &received)
      }),
    );
  }

  /// Check if a handler is registered for an RPC type.
  pub fn is_registered<T: Rpc>(&self) -> bool {
    self.type_to_id.contains_key(&TypeId::of::<T>())
  }

  /// Check if a handler exists for an RPC type ID.
  pub fn has_handler(&self, rpc_id: RpcTypeId) -> bool {
    self.handlers.contains_key(&rpc_id)
  }

  /// Get the name of an RPC type.
  pub fn get_name(&self, rpc_id: RpcTypeId) -> Option<&'static str> {
    self.id_to_name.get(&rpc_id).copied()
  }

  /// Dispatch an RPC to its handler.
  ///
  /// Returns an error if no handler is registered for the RPC type.
  pub fn dispatch(&self, world: &mut World, rpc: &SerializedRpc) -> Result<(), ReplicationError> {
    let handler = self.handlers.get(&rpc.rpc_id).ok_or_else(|| {
      ReplicationError::RpcNotFound(
        self
          .id_to_name
          .get(&rpc.rpc_id)
          .map(|s| s.to_string())
          .unwrap_or_else(|| format!("RpcTypeId({})", rpc.rpc_id.0)),
      )
    })?;

    handler(world, &rpc.payload, rpc.sender)
  }

  /// Get all registered RPC type IDs.
  pub fn registered_types(&self) -> impl Iterator<Item = RpcTypeId> + '_ {
    self.handlers.keys().copied()
  }

  /// Get the number of registered handlers.
  pub fn handler_count(&self) -> usize {
    self.handlers.len()
  }
}

impl Default for RpcRegistry {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::super::types::RpcTarget;
  use super::*;
  use serde::{Deserialize, Serialize};
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};

  #[derive(Debug, Clone, Serialize, Deserialize)]
  struct TestRpc {
    value: i32,
  }

  impl Rpc for TestRpc {
    fn rpc_id() -> RpcTypeId {
      RpcTypeId::from_name("TestRpc")
    }

    fn rpc_name() -> &'static str {
      "TestRpc"
    }
  }

  #[test]
  fn registry_register_and_check() {
    let mut registry = RpcRegistry::new();
    assert!(!registry.is_registered::<TestRpc>());

    registry.register::<TestRpc, _>(|_world, _rpc| Ok(()));

    assert!(registry.is_registered::<TestRpc>());
    assert!(registry.has_handler(TestRpc::rpc_id()));
    assert_eq!(registry.handler_count(), 1);
  }

  #[test]
  fn registry_get_name() {
    let mut registry = RpcRegistry::new();
    registry.register::<TestRpc, _>(|_world, _rpc| Ok(()));

    assert_eq!(registry.get_name(TestRpc::rpc_id()), Some("TestRpc"));
  }

  #[test]
  fn registry_dispatch() {
    let mut registry = RpcRegistry::new();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    registry.register::<TestRpc, _>(move |_world, rpc| {
      assert_eq!(rpc.payload.value, 42);
      assert_eq!(rpc.sender, 1);
      called_clone.store(true, Ordering::SeqCst);
      Ok(())
    });

    let serialized = SerializedRpc::from_rpc(&TestRpc { value: 42 }, 1, RpcTarget::Server).unwrap();

    let mut world = World::new();
    registry.dispatch(&mut world, &serialized).unwrap();

    assert!(called.load(Ordering::SeqCst));
  }

  #[test]
  fn registry_dispatch_unknown_rpc() {
    let registry = RpcRegistry::new();
    let serialized = SerializedRpc::from_rpc(&TestRpc { value: 42 }, 1, RpcTarget::Server).unwrap();

    let mut world = World::new();
    let result = registry.dispatch(&mut world, &serialized);

    assert!(result.is_err());
  }
}
