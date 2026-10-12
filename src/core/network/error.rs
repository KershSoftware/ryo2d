//! Networking and replication errors.

use thiserror::Error;

/// Network system errors
#[derive(Error, Debug)]
pub enum NetworkError {
  /// Failed to initialize networking
  #[error("Network initialization failed: {0}")]
  InitError(String),

  /// Connection failed
  #[error("Connection failed: {0}")]
  ConnectionError(String),

  /// Failed to send message
  #[error("Failed to send message: {0}")]
  SendError(String),

  /// Failed to receive message
  #[error("Failed to receive message: {0}")]
  ReceiveError(String),

  /// Transport layer error
  #[error("Transport error: {0}")]
  TransportError(String),

  /// Client is not connected
  #[error("Client not connected")]
  NotConnected,

  /// Server is not running
  #[error("Server not running")]
  NotRunning,
}

/// Result type for network operations
pub type NetworkResult<T> = Result<T, NetworkError>;

/// Replication system errors
#[derive(Error, Debug)]
pub enum ReplicationError {
  /// Failed to serialize component data
  #[error("Serialization failed: {0}")]
  SerializationError(String),

  /// Failed to deserialize component data
  #[error("Deserialization failed: {0}")]
  DeserializationError(String),

  /// Component type mismatch during deserialization
  #[error("Type mismatch: expected {expected}, found {found}")]
  TypeMismatch { expected: String, found: String },

  /// Unknown component type ID
  #[error("Unknown component type: ComponentTypeId({0})")]
  UnknownComponent(super::replication::ComponentTypeId),

  /// Entity not found in world
  #[error("Entity not found: NetworkId({0})")]
  EntityNotFound(u64),

  /// Network ID not found in mapping
  #[error("Network ID not found: {0}")]
  NetworkIdNotFound(super::replication::NetworkId),

  /// Authority violation (client tried to modify server-owned entity)
  #[error("Authority violation: client {client_id} cannot modify entity owned by {owner}")]
  AuthorityViolation { client_id: u64, owner: u64 },

  /// RPC handler not found
  #[error("RPC handler not found: {0}")]
  RpcNotFound(String),

  /// RPC execution failed
  #[error("RPC execution failed: {0}")]
  RpcError(String),

  /// Network send/receive error
  #[error("Network error: {0}")]
  NetworkError(String),
}

/// Result type for replication operations
pub type ReplicationResult<T> = Result<T, ReplicationError>;
