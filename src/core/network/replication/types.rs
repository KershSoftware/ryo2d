//! Core replication types for network entity management
//!
//! This module provides the fundamental types for network entity replication:
//! - [`NetworkId`] - Unique identifier for networked entities
//! - [`NetworkEntity`] - Component marking an entity as networked
//! - [`Authority`] - Who controls an entity (server or specific client)

use renet::ClientId;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Global counter for generating unique network IDs
static NEXT_NETWORK_ID: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for networked entities.
///
/// Each networked entity receives a globally unique `NetworkId` when spawned.
/// This ID is consistent across server and all clients, allowing entities
/// to be referenced by network messages.
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
///
/// let id = NetworkId::new();
/// println!("Entity network ID: {}", id.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetworkId(pub u64);

impl NetworkId {
  /// Generate a new unique network ID.
  ///
  /// IDs are monotonically increasing and guaranteed unique within
  /// a single process. In networked games, only the server should
  /// generate network IDs.
  pub fn new() -> Self {
    NetworkId(NEXT_NETWORK_ID.fetch_add(1, Ordering::Relaxed))
  }

  /// Create a network ID from a known value.
  ///
  /// Used when receiving entity spawn messages from the server.
  pub fn from_raw(id: u64) -> Self {
    NetworkId(id)
  }

  /// Get the raw u64 value.
  pub fn raw(&self) -> u64 {
    self.0
  }
}

impl Default for NetworkId {
  fn default() -> Self {
    Self::new()
  }
}

impl std::fmt::Display for NetworkId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "NetworkId({})", self.0)
  }
}

/// Authority determines who can modify an entity.
///
/// In a host-authoritative model:
/// - [`Authority::Server`] - Only the server can modify (default for most entities)
/// - [`Authority::Client`] - A specific client controls the entity (player characters)
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
///
/// // NPC controlled by server
/// let npc_authority = Authority::Server;
///
/// // Player controlled by client 5
/// let player_authority = Authority::Client(5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Authority {
  /// Server has authority over this entity (default).
  /// Only the server can modify component values.
  #[default]
  Server,

  /// A specific client has authority over this entity.
  /// Used for player-controlled entities where the client
  /// sends authoritative input/state.
  Client(ClientId),
}

/// Component marking an entity as networked.
///
/// Attach this component to any entity that should be synchronized
/// across the network. The replication system will automatically
/// sync components that implement [`Replicated`] on entities with
/// this component.
///
/// # Fields
///
/// - `network_id` - Unique identifier used in network messages
/// - `owner` - The client that "owns" this entity (for spawning/despawning)
/// - `authority` - Who can modify this entity's replicated components
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
///
/// // Create a server-controlled entity
/// let network_entity = NetworkEntity {
///     network_id: NetworkId::new(),
///     owner: 0, // Server owns it
///     authority: Authority::Server,
/// };
///
/// // Create a player-controlled entity
/// let player_entity = NetworkEntity {
///     network_id: NetworkId::new(),
///     owner: client_id,
///     authority: Authority::Client(client_id),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntity {
  /// Unique network identifier for this entity.
  pub network_id: NetworkId,

  /// The client that owns this entity.
  /// For server-spawned entities, this is typically 0 or the server's ID.
  /// For client-requested entities, this is the requesting client's ID.
  pub owner: ClientId,

  /// Who has authority to modify this entity.
  pub authority: Authority,
}

impl NetworkEntity {
  /// Create a new server-owned, server-authoritative network entity.
  pub fn new_server_owned() -> Self {
    Self {
      network_id: NetworkId::new(),
      owner: 0,
      authority: Authority::Server,
    }
  }

  /// Create a new client-owned network entity.
  ///
  /// By default, the client also has authority over the entity.
  pub fn new_client_owned(client_id: ClientId) -> Self {
    Self {
      network_id: NetworkId::new(),
      owner: client_id,
      authority: Authority::Client(client_id),
    }
  }

  /// Check if this entity is owned by the server.
  pub fn is_server_owned(&self) -> bool {
    self.owner == 0
  }

  /// Check if a specific client has authority over this entity.
  pub fn has_authority(&self, client_id: ClientId) -> bool {
    match self.authority {
      Authority::Server => client_id == 0,
      Authority::Client(auth_id) => client_id == auth_id,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn network_id_uniqueness() {
    let id1 = NetworkId::new();
    let id2 = NetworkId::new();
    let id3 = NetworkId::new();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_ne!(id1, id3);
  }

  #[test]
  fn network_id_from_raw() {
    let id = NetworkId::from_raw(42);
    assert_eq!(id.raw(), 42);
    assert_eq!(id.0, 42);
  }

  #[test]
  fn authority_default_is_server() {
    assert_eq!(Authority::default(), Authority::Server);
  }

  #[test]
  fn network_entity_server_owned() {
    let entity = NetworkEntity::new_server_owned();
    assert!(entity.is_server_owned());
    assert_eq!(entity.authority, Authority::Server);
    assert!(entity.has_authority(0));
    assert!(!entity.has_authority(1));
  }

  #[test]
  fn network_entity_client_owned() {
    let client_id = 5;
    let entity = NetworkEntity::new_client_owned(client_id);

    assert!(!entity.is_server_owned());
    assert_eq!(entity.owner, client_id);
    assert_eq!(entity.authority, Authority::Client(client_id));
    assert!(entity.has_authority(client_id));
    assert!(!entity.has_authority(0));
    assert!(!entity.has_authority(6));
  }

  #[test]
  fn network_entity_serialization() {
    let entity = NetworkEntity::new_server_owned();
    let json = serde_json::to_string(&entity).unwrap();
    let deserialized: NetworkEntity = serde_json::from_str(&json).unwrap();

    assert_eq!(entity.network_id, deserialized.network_id);
    assert_eq!(entity.owner, deserialized.owner);
    assert_eq!(entity.authority, deserialized.authority);
  }
}
