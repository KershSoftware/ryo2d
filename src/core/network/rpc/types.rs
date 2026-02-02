//! RPC types for remote procedure calls
//!
//! Defines the core types for the RPC system.

use renet::ClientId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

/// Unique identifier for an RPC type.
///
/// Similar to `ComponentTypeId`, this is derived from the type name
/// for deterministic identification across server and clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RpcTypeId(pub u32);

impl RpcTypeId {
  /// Create an RPC type ID from a type name.
  ///
  /// Uses FNV-1a hash for deterministic, fast hashing.
  pub fn from_name(name: &str) -> Self {
    // FNV-1a hash
    let mut hash: u32 = 2166136261;
    for byte in name.bytes() {
      hash ^= byte as u32;
      hash = hash.wrapping_mul(16777619);
    }
    RpcTypeId(hash)
  }

  /// Create from a raw value.
  pub fn from_raw(id: u32) -> Self {
    RpcTypeId(id)
  }

  /// Get the raw u32 value.
  pub fn raw(&self) -> u32 {
    self.0
  }
}

impl std::fmt::Display for RpcTypeId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "RpcTypeId({})", self.0)
  }
}

/// Trait for types that can be sent as RPCs.
///
/// Implement this trait for any struct that should be callable
/// as a remote procedure.
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct PlayerInputRpc {
///     x: f32,
///     y: f32,
///     jump: bool,
/// }
///
/// impl Rpc for PlayerInputRpc {
///     fn rpc_id() -> RpcTypeId {
///         RpcTypeId::from_name("PlayerInputRpc")
///     }
///
///     fn rpc_name() -> &'static str {
///         "PlayerInputRpc"
///     }
/// }
/// ```
pub trait Rpc: Serialize + DeserializeOwned + Clone + Send + Sync + 'static {
  /// Get the unique type ID for this RPC.
  fn rpc_id() -> RpcTypeId;

  /// Get the human-readable name of this RPC.
  fn rpc_name() -> &'static str;
}

/// Target for an RPC message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcTarget {
  /// Send to server only (client → server)
  Server,

  /// Send to a specific client (server → client)
  Client(ClientId),

  /// Broadcast to all clients (server → all)
  Broadcast,

  /// Broadcast to all clients except one (server → all except sender)
  BroadcastExcept(ClientId),
}

/// A received RPC with sender information.
#[derive(Debug, Clone)]
pub struct ReceivedRpc<T: Rpc> {
  /// The RPC payload
  pub payload: T,
  /// Who sent this RPC (0 for server)
  pub sender: ClientId,
}

impl<T: Rpc> ReceivedRpc<T> {
  /// Create a new received RPC.
  pub fn new(payload: T, sender: ClientId) -> Self {
    Self { payload, sender }
  }

  /// Check if the RPC was sent by the server.
  pub fn is_from_server(&self) -> bool {
    self.sender == 0
  }
}

/// Serialized RPC for network transmission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedRpc {
  /// The RPC type identifier
  pub rpc_id: RpcTypeId,
  /// The RPC name (for debugging)
  pub name: String,
  /// Serialized payload
  pub payload: Vec<u8>,
  /// Who sent this RPC
  pub sender: ClientId,
  /// Target of this RPC
  pub target: RpcTarget,
}

impl SerializedRpc {
  /// Create a serialized RPC from a typed RPC.
  pub fn from_rpc<T: Rpc>(
    rpc: &T,
    sender: ClientId,
    target: RpcTarget,
  ) -> Result<Self, serde_json::Error> {
    Ok(Self {
      rpc_id: T::rpc_id(),
      name: T::rpc_name().to_string(),
      payload: serde_json::to_vec(rpc)?,
      sender,
      target,
    })
  }

  /// Deserialize into a typed RPC.
  pub fn into_rpc<T: Rpc>(self) -> Result<ReceivedRpc<T>, serde_json::Error> {
    let payload: T = serde_json::from_slice(&self.payload)?;
    Ok(ReceivedRpc::new(payload, self.sender))
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

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
  fn rpc_type_id_deterministic() {
    let id1 = RpcTypeId::from_name("TestRpc");
    let id2 = RpcTypeId::from_name("TestRpc");
    let id3 = RpcTypeId::from_name("OtherRpc");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
  }

  #[test]
  fn serialized_rpc_roundtrip() {
    let rpc = TestRpc { value: 42 };
    let serialized = SerializedRpc::from_rpc(&rpc, 1, RpcTarget::Server).unwrap();

    assert_eq!(serialized.rpc_id, TestRpc::rpc_id());
    assert_eq!(serialized.name, "TestRpc");
    assert_eq!(serialized.sender, 1);

    let received: ReceivedRpc<TestRpc> = serialized.into_rpc().unwrap();
    assert_eq!(received.payload.value, 42);
    assert_eq!(received.sender, 1);
  }

  #[test]
  fn rpc_target_variants() {
    assert_eq!(RpcTarget::Server, RpcTarget::Server);
    assert_ne!(RpcTarget::Server, RpcTarget::Broadcast);
    assert_eq!(RpcTarget::Client(5), RpcTarget::Client(5));
    assert_ne!(RpcTarget::Client(5), RpcTarget::Client(6));
  }
}
