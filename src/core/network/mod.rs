//! Networking module for Ryo2D
//!
//! Provides client-server networking using renet as the foundation.
//! This module wraps renet's low-level API with a cleaner interface
//! suitable for game development.
//!
//! # Server Types
//!
//! - [`NetworkServer`] - Dedicated server (no local player)
//! - [`ListenServer`] - Player-hosted server (host plays while serving)
//! - [`NetworkClient`] - Connects to either server type
//!
//! # Replication
//!
//! The [`replication`] submodule provides automatic entity/component
//! synchronization between server and clients.
//!
//! # Quick Start
//!
//! ## Dedicated Server
//!
//! ```no_run
//! use ryo2d::prelude::*;
//! use std::time::Duration;
//!
//! let config = NetworkServerConfig {
//!     port: 5000,
//!     max_clients: 16,
//!     protocol_id: 12345,
//! };
//! let mut server = NetworkServer::new(config).unwrap();
//!
//! // Game loop
//! let dt = Duration::from_secs_f32(1.0 / 60.0);
//! server.update(dt);
//!
//! for event in server.events() {
//!     match event {
//!         NetworkEvent::ClientConnected { client_id } => {
//!             println!("Client {} connected", client_id);
//!         }
//!         NetworkEvent::ClientDisconnected { client_id, reason } => {
//!             println!("Client {} disconnected: {}", client_id, reason);
//!         }
//!         _ => {}
//!     }
//! }
//! ```
//!
//! ## Listen Server (Player-Hosted)
//!
//! For casual co-op games where one player hosts:
//!
//! ```no_run
//! use ryo2d::prelude::*;
//! use std::time::Duration;
//!
//! let config = NetworkServerConfig {
//!     port: 5000,
//!     max_clients: 3, // 3 remote + host = 4 players
//!     protocol_id: 12345,
//! };
//! let mut server = ListenServer::new(config).unwrap();
//!
//! // Game loop
//! server.update(Duration::from_secs_f32(1.0 / 60.0));
//!
//! // Handle remote client events
//! for event in server.events() {
//!     // Only remote clients generate events
//! }
//!
//! // Iterate all players uniformly
//! for client_id in server.all_clients() {
//!     if server.is_host(client_id) {
//!         // Host player - handle directly
//!     } else {
//!         // Remote player - receive from network
//!         while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
//!             // Process remote input
//!         }
//!     }
//! }
//!
//! // Broadcast to remote clients (host's state is local)
//! server.broadcast(Channel::Unreliable, b"game state");
//! ```
//!
//! ## Client
//!
//! ```no_run
//! use ryo2d::prelude::*;
//! use std::net::SocketAddr;
//! use std::time::Duration;
//!
//! let server_addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
//! let config = NetworkClientConfig {
//!     server_addr,
//!     protocol_id: 12345, // Must match server
//! };
//! let mut client = NetworkClient::connect(config).unwrap();
//!
//! // Game loop
//! client.update(Duration::from_secs_f32(1.0 / 60.0));
//!
//! for event in client.events() {
//!     match event {
//!         NetworkEvent::Connected => println!("Connected!"),
//!         NetworkEvent::Disconnected { reason } => println!("Lost connection: {}", reason),
//!         _ => {}
//!     }
//! }
//!
//! // Send input to server
//! client.send(Channel::ReliableOrdered, b"jump");
//!
//! // Receive game state
//! while let Some(data) = client.receive(Channel::Unreliable) {
//!     // Update local state
//! }
//! ```

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

use renet::{ConnectionConfig, DefaultChannel, RenetClient, RenetServer, ServerEvent};
use renet_netcode::{
  ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
  ServerConfig,
};

use super::error::{NetworkError, NetworkResult};

pub mod replication;
pub mod rpc;

// Re-export ClientId for convenience
pub use renet::ClientId;

/// Trait for abstracting over different server types.
///
/// This trait allows code to work with both [`NetworkServer`] and
/// [`ListenServer`] without knowing which one is being used.
///
/// # Example
///
/// ```no_run
/// use ryo2d::prelude::*;
///
/// fn broadcast_game_state<S: ServerLike>(server: &mut S, data: &[u8]) {
///     server.broadcast(Channel::Unreliable, data);
/// }
/// ```
pub trait ServerLike {
    /// Send a message to a specific client.
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]);

    /// Broadcast a message to all clients.
    fn broadcast(&mut self, channel: Channel, data: &[u8]);

    /// Receive a message from a specific client.
    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>>;

    /// Get an iterator over all client IDs.
    ///
    /// For `ListenServer`, this includes the host. For `NetworkServer`, this
    /// is only remote clients.
    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_>;

    /// Check if a client ID is the local host.
    ///
    /// Returns `true` for `ListenServer`'s host player, `false` otherwise.
    fn is_host(&self, client_id: ClientId) -> bool;
}

/// Reserved client ID for the local host player in a listen server.
///
/// This constant is used to represent the host player who runs the server
/// and also participates in the game. The host's messages are handled
/// directly in game logic without going through the network.
///
/// This ID (0) will never be assigned to a remote client by renet,
/// making it safe to use as a sentinel value for the local host.
pub const LOCAL_HOST_ID: ClientId = 0;

/// Channel types for message delivery
///
/// These map to renet's DefaultChannel variants but provide a cleaner API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
  /// Best-effort delivery, no ordering guarantees.
  /// Use for frequently-updated data like position updates where
  /// missing a packet is acceptable.
  Unreliable,

  /// Guaranteed delivery with ordering.
  /// Messages arrive in the order they were sent.
  /// Use for game events, chat messages, and state changes.
  ReliableOrdered,

  /// Guaranteed delivery without ordering.
  /// Messages are guaranteed to arrive but may be reordered.
  /// Use for independent events that don't depend on order.
  ReliableUnordered,
}

impl From<Channel> for DefaultChannel {
  fn from(channel: Channel) -> Self {
    match channel {
      Channel::Unreliable => DefaultChannel::Unreliable,
      Channel::ReliableOrdered => DefaultChannel::ReliableOrdered,
      Channel::ReliableUnordered => DefaultChannel::ReliableUnordered,
    }
  }
}

/// Network events for game logic
///
/// These events are generated by the networking layer and should be
/// processed each frame in your game's update loop.
#[derive(Debug, Clone)]
pub enum NetworkEvent {
  /// A client connected to the server (server-side only)
  ClientConnected {
    /// The unique identifier of the connected client
    client_id: ClientId,
  },

  /// A client disconnected from the server (server-side only)
  ClientDisconnected {
    /// The unique identifier of the disconnected client
    client_id: ClientId,
    /// Human-readable reason for disconnection
    reason: String,
  },

  /// Client successfully connected to server (client-side only)
  Connected,

  /// Client disconnected from server (client-side only)
  Disconnected {
    /// Human-readable reason for disconnection
    reason: String,
  },
}

/// Configuration for creating a network server
#[derive(Debug, Clone)]
pub struct NetworkServerConfig {
  /// Port to listen on (default: 5000)
  pub port: u16,

  /// Maximum number of simultaneous clients (default: 16)
  pub max_clients: usize,

  /// Protocol ID - must match between client and server (default: 0)
  /// Use different IDs for different game versions to prevent mismatches.
  pub protocol_id: u64,
}

impl Default for NetworkServerConfig {
  fn default() -> Self {
    Self {
      port: 5000,
      max_clients: 16,
      protocol_id: 0,
    }
  }
}

/// A network server that manages client connections
///
/// Wraps renet's `RenetServer` and `NetcodeServerTransport` with a
/// simpler API. Create one server per game session.
pub struct NetworkServer {
  server: RenetServer,
  transport: NetcodeServerTransport,
  events: Vec<NetworkEvent>,
}

impl NetworkServer {
  /// Create and start a new server
  ///
  /// Binds to `0.0.0.0:{port}` to accept connections from any interface.
  ///
  /// # Errors
  ///
  /// Returns `NetworkError::InitError` if socket binding or transport
  /// creation fails.
  pub fn new(config: NetworkServerConfig) -> NetworkResult<Self> {
    let server_addr: SocketAddr = format!("0.0.0.0:{}", config.port)
      .parse()
      .map_err(|e| NetworkError::InitError(format!("Invalid address: {}", e)))?;

    let socket = UdpSocket::bind(server_addr)
      .map_err(|e| NetworkError::InitError(format!("Failed to bind socket: {}", e)))?;

    let current_time = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or(Duration::ZERO);

    let server_config = ServerConfig {
      current_time,
      max_clients: config.max_clients,
      protocol_id: config.protocol_id,
      public_addresses: vec![server_addr],
      authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket)
      .map_err(|e| NetworkError::InitError(format!("Failed to create transport: {}", e)))?;

    let connection_config = ConnectionConfig::default();
    let server = RenetServer::new(connection_config);

    Ok(Self {
      server,
      transport,
      events: Vec::new(),
    })
  }

  /// Update server state
  ///
  /// Must be called each frame with the frame's delta time.
  /// This processes incoming packets and generates events.
  pub fn update(&mut self, dt: Duration) {
    self.server.update(dt);

    if let Err(e) = self.transport.update(dt, &mut self.server) {
      // Transport errors are logged but don't crash the server
      eprintln!("Network transport error: {}", e);
    }

    // Collect server events
    while let Some(event) = self.server.get_event() {
      match event {
        ServerEvent::ClientConnected { client_id } => {
          self
            .events
            .push(NetworkEvent::ClientConnected { client_id });
        }
        ServerEvent::ClientDisconnected { client_id, reason } => {
          self.events.push(NetworkEvent::ClientDisconnected {
            client_id,
            reason: format!("{:?}", reason),
          });
        }
      }
    }

    // Send outgoing packets
    self.transport.send_packets(&mut self.server);
  }

  /// Get pending network events
  ///
  /// Events are cleared after iteration. Call this each frame after `update()`.
  pub fn events(&mut self) -> impl Iterator<Item = NetworkEvent> + '_ {
    self.events.drain(..)
  }

  /// Send a message to a specific client
  ///
  /// The message is queued and sent during the next `update()` call.
  pub fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]) {
    self
      .server
      .send_message(client_id, DefaultChannel::from(channel), data.to_vec());
  }

  /// Broadcast a message to all connected clients
  pub fn broadcast(&mut self, channel: Channel, data: &[u8]) {
    self
      .server
      .broadcast_message(DefaultChannel::from(channel), data.to_vec());
  }

  /// Broadcast a message to all clients except one
  ///
  /// Useful for relaying messages from one client to all others.
  pub fn broadcast_except(&mut self, exclude: ClientId, channel: Channel, data: &[u8]) {
    let clients: Vec<ClientId> = self.clients().collect();
    for client_id in clients {
      if client_id != exclude {
        self.send(client_id, channel, data);
      }
    }
  }

  /// Receive a message from a specific client
  ///
  /// Returns `None` if no message is available on the specified channel.
  /// Call repeatedly until `None` to drain all messages.
  pub fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>> {
    self
      .server
      .receive_message(client_id, DefaultChannel::from(channel))
      .map(|bytes| bytes.to_vec())
  }

  /// Get an iterator over all connected client IDs
  pub fn clients(&self) -> impl Iterator<Item = ClientId> + '_ {
    self.server.clients_id_iter()
  }

  /// Disconnect a specific client
  pub fn disconnect(&mut self, client_id: ClientId) {
    self.server.disconnect(client_id);
  }

  /// Check if the server is running
  ///
  /// Returns `true` if the server was successfully created.
  pub fn is_running(&self) -> bool {
    // Server is always running if it was successfully created
    true
  }

  /// Get the number of connected clients
  pub fn connected_count(&self) -> usize {
    self.server.connected_clients()
  }
}

impl ServerLike for NetworkServer {
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]) {
        NetworkServer::send(self, client_id, channel, data);
    }

    fn broadcast(&mut self, channel: Channel, data: &[u8]) {
        NetworkServer::broadcast(self, channel, data);
    }

    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>> {
        NetworkServer::receive(self, client_id, channel)
    }

    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_> {
        Box::new(NetworkServer::clients(self))
    }

    fn is_host(&self, _client_id: ClientId) -> bool {
        // NetworkServer has no host - all clients are remote
        false
    }
}

/// Configuration for connecting to a network server
#[derive(Debug, Clone)]
pub struct NetworkClientConfig {
  /// Server address to connect to
  pub server_addr: SocketAddr,

  /// Protocol ID - must match the server (default: 0)
  pub protocol_id: u64,
}

/// A network client that connects to a server
///
/// Wraps renet's `RenetClient` and `NetcodeClientTransport` with a
/// simpler API. Create one client per game session.
pub struct NetworkClient {
  client: RenetClient,
  transport: NetcodeClientTransport,
  events: Vec<NetworkEvent>,
  was_connected: bool,
}

impl NetworkClient {
  /// Connect to a server
  ///
  /// The connection is established asynchronously. Check `is_connected()`
  /// or wait for `NetworkEvent::Connected` to know when the connection
  /// is established.
  ///
  /// # Errors
  ///
  /// Returns `NetworkError::ConnectionError` if socket creation fails.
  pub fn connect(config: NetworkClientConfig) -> NetworkResult<Self> {
    // Bind to any available port
    let socket = UdpSocket::bind("0.0.0.0:0")
      .map_err(|e| NetworkError::ConnectionError(format!("Failed to bind socket: {}", e)))?;

    let current_time = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or(Duration::ZERO);

    // Generate a random client ID
    let client_id = rand_u64();

    let authentication = ClientAuthentication::Unsecure {
      protocol_id: config.protocol_id,
      client_id,
      server_addr: config.server_addr,
      user_data: None,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket)
      .map_err(|e| NetworkError::ConnectionError(format!("Failed to create transport: {}", e)))?;

    let connection_config = ConnectionConfig::default();
    let client = RenetClient::new(connection_config);

    Ok(Self {
      client,
      transport,
      events: Vec::new(),
      was_connected: false,
    })
  }

  /// Update client state
  ///
  /// Must be called each frame with the frame's delta time.
  /// This processes incoming packets and generates events.
  pub fn update(&mut self, dt: Duration) {
    self.client.update(dt);

    if let Err(e) = self.transport.update(dt, &mut self.client) {
      eprintln!("Network transport error: {}", e);
    }

    // Check for connection state changes
    let is_connected = self.client.is_connected();

    if is_connected && !self.was_connected {
      self.events.push(NetworkEvent::Connected);
    } else if !is_connected && self.was_connected {
      let reason = self
        .transport
        .disconnect_reason()
        .map(|r| format!("{:?}", r))
        .unwrap_or_else(|| "Unknown".to_string());
      self.events.push(NetworkEvent::Disconnected { reason });
    }

    self.was_connected = is_connected;

    // Send outgoing packets
    if let Err(e) = self.transport.send_packets(&mut self.client) {
      eprintln!("Network send error: {}", e);
    }
  }

  /// Get pending network events
  ///
  /// Events are cleared after iteration. Call this each frame after `update()`.
  pub fn events(&mut self) -> impl Iterator<Item = NetworkEvent> + '_ {
    self.events.drain(..)
  }

  /// Send a message to the server
  ///
  /// The message is queued and sent during the next `update()` call.
  pub fn send(&mut self, channel: Channel, data: &[u8]) {
    self
      .client
      .send_message(DefaultChannel::from(channel), data.to_vec());
  }

  /// Receive a message from the server
  ///
  /// Returns `None` if no message is available on the specified channel.
  /// Call repeatedly until `None` to drain all messages.
  pub fn receive(&mut self, channel: Channel) -> Option<Vec<u8>> {
    self
      .client
      .receive_message(DefaultChannel::from(channel))
      .map(|bytes| bytes.to_vec())
  }

  /// Check if connected to the server
  pub fn is_connected(&self) -> bool {
    self.client.is_connected()
  }

  /// Check if currently connecting to the server
  pub fn is_connecting(&self) -> bool {
    self.client.is_connecting()
  }

  /// Check if disconnected from the server
  pub fn is_disconnected(&self) -> bool {
    self.client.is_disconnected()
  }

  /// Disconnect from the server
  pub fn disconnect(&mut self) {
    self.transport.disconnect();
  }

  /// Get the disconnect reason if disconnected
  pub fn disconnect_reason(&self) -> Option<String> {
    self.transport.disconnect_reason().map(|r| format!("{:?}", r))
  }

  /// Get the client's unique ID
  pub fn client_id(&self) -> ClientId {
    self.transport.client_id()
  }
}

/// A network server where the host is also a player.
///
/// Use this for player-hosted games where one player runs the server
/// and also plays in the game. Remote clients connect normally via
/// `NetworkClient`.
///
/// The host player's messages are NOT part of the networking system -
/// the host's input/state is handled directly in game logic. The
/// `ListenServer` only manages remote client connections.
///
/// # Example
///
/// ```no_run
/// use ryo2d::prelude::*;
/// use std::time::Duration;
///
/// let config = NetworkServerConfig {
///     port: 5000,
///     max_clients: 4,
///     protocol_id: 12345,
/// };
/// let mut server = ListenServer::new(config).unwrap();
///
/// // Game loop
/// let dt = Duration::from_secs_f32(1.0 / 60.0);
/// server.update(dt);
///
/// // Handle remote client events
/// for event in server.events() {
///     match event {
///         NetworkEvent::ClientConnected { client_id } => {
///             println!("Remote client {} connected", client_id);
///         }
///         NetworkEvent::ClientDisconnected { client_id, reason } => {
///             println!("Remote client {} disconnected: {}", client_id, reason);
///         }
///         _ => {}
///     }
/// }
///
/// // Iterate all players (host + remote)
/// for client_id in server.all_clients() {
///     if server.is_host(client_id) {
///         // Host player - handle directly
///     } else {
///         // Remote player - receive messages
///         while let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
///             // Process remote input
///         }
///     }
/// }
/// ```
pub struct ListenServer {
  server: NetworkServer,
  host_id: ClientId,
}

impl ListenServer {
  /// Create a new listen server.
  ///
  /// The host player is automatically assigned `LOCAL_HOST_ID`.
  ///
  /// # Errors
  ///
  /// Returns `NetworkError::InitError` if socket binding or transport
  /// creation fails.
  pub fn new(config: NetworkServerConfig) -> NetworkResult<Self> {
    let server = NetworkServer::new(config)?;
    Ok(Self {
      server,
      host_id: LOCAL_HOST_ID,
    })
  }

  /// The host player's client ID (always `LOCAL_HOST_ID`).
  pub fn host_id(&self) -> ClientId {
    self.host_id
  }

  /// Update network state.
  ///
  /// Must be called each frame with the frame's delta time.
  /// This processes incoming packets and generates events for remote clients.
  pub fn update(&mut self, dt: Duration) {
    self.server.update(dt);
  }

  /// Get pending network events.
  ///
  /// Events are about remote clients only (connected, disconnected).
  /// Events are cleared after iteration. Call this each frame after `update()`.
  pub fn events(&mut self) -> impl Iterator<Item = NetworkEvent> + '_ {
    self.server.events()
  }

  /// Iterator over remote client IDs only.
  ///
  /// Does not include the host player.
  pub fn remote_clients(&self) -> impl Iterator<Item = ClientId> + '_ {
    self.server.clients()
  }

  /// Iterator over all clients (remote + host).
  ///
  /// Useful for game logic that treats all players uniformly.
  /// The host is always included first.
  pub fn all_clients(&self) -> impl Iterator<Item = ClientId> + '_ {
    std::iter::once(self.host_id).chain(self.server.clients())
  }

  /// Check if a client ID is the local host.
  pub fn is_host(&self, client_id: ClientId) -> bool {
    client_id == self.host_id
  }

  /// Send a message to a specific remote client.
  ///
  /// Does nothing if `client_id` is the host (use direct game logic instead).
  /// The message is queued and sent during the next `update()` call.
  pub fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]) {
    if !self.is_host(client_id) {
      self.server.send(client_id, channel, data);
    }
  }

  /// Broadcast a message to all remote clients.
  ///
  /// Does not send to the host (the host's state is local).
  pub fn broadcast(&mut self, channel: Channel, data: &[u8]) {
    self.server.broadcast(channel, data);
  }

  /// Broadcast a message to all remote clients except one.
  ///
  /// If `exclude` is the host, this is equivalent to `broadcast()`.
  pub fn broadcast_except(&mut self, exclude: ClientId, channel: Channel, data: &[u8]) {
    if self.is_host(exclude) {
      // Excluding host means broadcast to all remotes
      self.server.broadcast(channel, data);
    } else {
      self.server.broadcast_except(exclude, channel, data);
    }
  }

  /// Receive a message from a specific remote client.
  ///
  /// Returns `None` if no message is available, or if `client_id` is the host.
  /// Call repeatedly until `None` to drain all messages.
  pub fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>> {
    if self.is_host(client_id) {
      None
    } else {
      self.server.receive(client_id, channel)
    }
  }

  /// Number of connected remote clients (not including host).
  pub fn remote_count(&self) -> usize {
    self.server.connected_count()
  }

  /// Total player count (remote clients + host).
  pub fn player_count(&self) -> usize {
    self.server.connected_count() + 1
  }

  /// Disconnect a remote client.
  ///
  /// Does nothing if `client_id` is the host.
  pub fn disconnect(&mut self, client_id: ClientId) {
    if !self.is_host(client_id) {
      self.server.disconnect(client_id);
    }
  }

  /// Check if the server is running.
  pub fn is_running(&self) -> bool {
    self.server.is_running()
  }
}

impl ServerLike for ListenServer {
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]) {
        ListenServer::send(self, client_id, channel, data);
    }

    fn broadcast(&mut self, channel: Channel, data: &[u8]) {
        ListenServer::broadcast(self, channel, data);
    }

    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>> {
        ListenServer::receive(self, client_id, channel)
    }

    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_> {
        Box::new(ListenServer::all_clients(self))
    }

    fn is_host(&self, client_id: ClientId) -> bool {
        ListenServer::is_host(self, client_id)
    }
}

/// Generate a random u64 using system randomness
fn rand_u64() -> u64 {
  use std::collections::hash_map::RandomState;
  use std::hash::{BuildHasher, Hasher};

  // Use RandomState which uses system randomness
  let state = RandomState::new();
  let mut hasher = state.build_hasher();
  hasher.write_u64(
    SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or(Duration::ZERO)
      .as_nanos() as u64,
  );
  hasher.finish()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn channel_conversion() {
    assert!(matches!(
      DefaultChannel::from(Channel::Unreliable),
      DefaultChannel::Unreliable
    ));
    assert!(matches!(
      DefaultChannel::from(Channel::ReliableOrdered),
      DefaultChannel::ReliableOrdered
    ));
    assert!(matches!(
      DefaultChannel::from(Channel::ReliableUnordered),
      DefaultChannel::ReliableUnordered
    ));
  }

  #[test]
  fn server_config_default() {
    let config = NetworkServerConfig::default();
    assert_eq!(config.port, 5000);
    assert_eq!(config.max_clients, 16);
    assert_eq!(config.protocol_id, 0);
  }

  #[test]
  fn network_event_debug() {
    let event = NetworkEvent::ClientConnected { client_id: 1 };
    assert!(format!("{:?}", event).contains("ClientConnected"));

    let event = NetworkEvent::Connected;
    assert!(format!("{:?}", event).contains("Connected"));
  }

  #[test]
  fn local_host_id_is_zero() {
    assert_eq!(LOCAL_HOST_ID, 0);
  }

  #[test]
  fn listen_server_host_id() {
    // Note: This test creates a real server, so it needs a free port
    let config = NetworkServerConfig {
      port: 19000,
      max_clients: 4,
      protocol_id: 12345,
    };

    if let Ok(server) = ListenServer::new(config) {
      assert_eq!(server.host_id(), LOCAL_HOST_ID);
    }
    // If port is in use, skip test silently
  }

  #[test]
  fn listen_server_is_host() {
    let config = NetworkServerConfig {
      port: 19001,
      max_clients: 4,
      protocol_id: 12345,
    };

    if let Ok(server) = ListenServer::new(config) {
      assert!(server.is_host(LOCAL_HOST_ID));
      assert!(!server.is_host(1));
      assert!(!server.is_host(999));
    }
  }

  #[test]
  fn listen_server_player_count_starts_at_one() {
    let config = NetworkServerConfig {
      port: 19002,
      max_clients: 4,
      protocol_id: 12345,
    };

    if let Ok(server) = ListenServer::new(config) {
      // Host is always counted
      assert_eq!(server.player_count(), 1);
      assert_eq!(server.remote_count(), 0);
    }
  }

  #[test]
  fn listen_server_all_clients_includes_host() {
    let config = NetworkServerConfig {
      port: 19003,
      max_clients: 4,
      protocol_id: 12345,
    };

    if let Ok(server) = ListenServer::new(config) {
      let clients: Vec<_> = server.all_clients().collect();
      assert_eq!(clients.len(), 1);
      assert_eq!(clients[0], LOCAL_HOST_ID);
    }
  }

  #[test]
  fn listen_server_is_running() {
    let config = NetworkServerConfig {
      port: 19004,
      max_clients: 4,
      protocol_id: 12345,
    };

    if let Ok(server) = ListenServer::new(config) {
      assert!(server.is_running());
    }
  }
}
