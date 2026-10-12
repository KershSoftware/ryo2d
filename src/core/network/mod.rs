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
//! # Transport Types
//!
//! - **UDP** (default) - Direct UDP sockets, lowest latency for LAN
//! - **WebRTC** (requires `matchbox` feature) - NAT traversal for internet games
//!
//! # Replication
//!
//! The [`replication`] submodule provides automatic entity/component
//! synchronization between server and clients.
//!
//! # Quick Start
//!
//! ## Dedicated Server (LAN)
//!
//! ```no_run
//! use ryo2d::prelude::*;
//! use std::time::Duration;
//!
//! let config = NetworkServerConfig {
//!     port: 5000,
//!     max_clients: 16,
//!     protocol_id: 12345,
//!     ..Default::default()  // Uses UDP transport
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
//! ## Internet Server (WebRTC)
//!
//! For games that need NAT traversal (requires `matchbox` feature).
//! You must run your own signaling server: `cargo install matchbox_server && matchbox_server`
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Local development (matchbox_server on localhost:3536)
//! let config = NetworkServerConfig {
//!     transport: TransportConfig::WebRtc(WebRtcConfig::new(
//!         "ws://localhost:3536",
//!         "my-game-room",
//!     )),
//!     ..Default::default()
//! };
//!
//! // Production (your deployed signaling server)
//! let config = NetworkServerConfig {
//!     transport: TransportConfig::WebRtc(WebRtcConfig::new(
//!         "wss://match.yourgame.com",
//!         "lobby-abc123",
//!     )),
//!     ..Default::default()
//! };
//! let mut server = NetworkServer::new(config).unwrap();
//! ```
//!
//! ## Listen Server (Player-Hosted)
//!
//! For casual co-op games where one player hosts:
//!
//! ```ignore
//! use ryo2d::prelude::*;
//! use std::time::Duration;
//!
//! let config = NetworkServerConfig {
//!     port: 5000,
//!     max_clients: 3, // 3 remote + host = 4 players
//!     protocol_id: 12345,
//!     ..Default::default()
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
//! let clients: Vec<_> = server.all_clients().collect();
//! for client_id in clients {
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
//! let config = NetworkClientConfig::udp(server_addr, 12345);
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

use std::net::SocketAddr;
use std::time::Duration;

use renet::{ConnectionConfig, DefaultChannel, RenetClient, RenetServer, ServerEvent};

pub mod action;
pub mod debug;
mod error;
pub mod input;
mod messages;
pub mod prediction;
pub mod replication;
pub mod rpc;
pub mod transport;

use transport::{ClientTransport, ServerTransport};

// Re-export ClientId for convenience
pub use error::{NetworkError, NetworkResult, ReplicationError, ReplicationResult};
pub use messages::{
  ClientConnected, ClientDisconnected, ConnectedToServer, DisconnectReason, DisconnectedFromServer,
};
pub use renet::ClientId;

// Re-export transport types
pub use transport::TransportConfig;

#[cfg(feature = "matchbox")]
pub use transport::{IceServer, WebRtcConfig};

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
/// The first three map to renet's DefaultChannel variants; the replication channels
/// are reserved for the engine.
///
/// New variants may be added; match with a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
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

  /// Reserved for [`ReplicationManager`](replication::ReplicationManager): spawns,
  /// despawns and full-state syncs. Don't send game messages on it.
  ReplicationReliable,

  /// Reserved for [`ReplicationManager`](replication::ReplicationManager): component
  /// changes, refreshes and acknowledgements. Don't send game messages on it.
  ReplicationUnreliable,
}

/// The renet channel ID for a `Channel` (renet's defaults plus the replication channels).
impl From<Channel> for u8 {
  fn from(channel: Channel) -> Self {
    match channel {
      Channel::Unreliable => DefaultChannel::Unreliable.into(),
      Channel::ReliableUnordered => DefaultChannel::ReliableUnordered.into(),
      Channel::ReliableOrdered => DefaultChannel::ReliableOrdered.into(),
      Channel::ReplicationReliable => 3,
      Channel::ReplicationUnreliable => 4,
    }
  }
}

/// The renet connection config every server and client uses: renet's three default
/// channels plus the two replication channels, in both directions.
pub(crate) fn connection_config() -> ConnectionConfig {
  let mut channels = DefaultChannel::config();
  channels.push(renet::ChannelConfig {
    channel_id: Channel::ReplicationReliable.into(),
    max_memory_usage_bytes: 5 * 1024 * 1024,
    send_type: renet::SendType::ReliableOrdered {
      resend_time: std::time::Duration::from_millis(300),
    },
  });
  channels.push(renet::ChannelConfig {
    channel_id: Channel::ReplicationUnreliable.into(),
    max_memory_usage_bytes: 5 * 1024 * 1024,
    send_type: renet::SendType::Unreliable,
  });
  ConnectionConfig {
    server_channels_config: channels.clone(),
    client_channels_config: channels,
    ..ConnectionConfig::default()
  }
}

/// The matchbox (WebRTC) channel index for a `Channel`.
#[cfg(feature = "matchbox")]
fn webrtc_channel(channel: Channel) -> usize {
  match channel {
    Channel::ReliableOrdered | Channel::ReliableUnordered => 0,
    Channel::Unreliable => 1,
    Channel::ReplicationReliable => 2,
    Channel::ReplicationUnreliable => 3,
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
  ///
  /// Only used for UDP transport. WebRTC uses signaling server instead.
  pub port: u16,

  /// Maximum number of simultaneous clients (default: 16)
  pub max_clients: usize,

  /// Protocol ID - must match between client and server (default: 0)
  /// Use different IDs for different game versions to prevent mismatches.
  pub protocol_id: u64,

  /// Transport configuration (default: UDP)
  ///
  /// Use `TransportConfig::Udp` for LAN games (lowest latency).
  /// Use `TransportConfig::WebRtc(...)` for internet games (NAT traversal).
  pub transport: TransportConfig,
}

impl NetworkServerConfig {
  /// Create a new server config with default values.
  pub fn new() -> Self {
    Self::default()
  }
}

impl Default for NetworkServerConfig {
  /// Port 5000, 16 clients, protocol ID 0, UDP.
  fn default() -> Self {
    Self {
      port: 5000,
      max_clients: 16,
      protocol_id: 0,
      transport: TransportConfig::Udp,
    }
  }
}

/// Internal enum for server transport implementations.
enum ServerTransportImpl {
  Udp(Box<transport::udp::UdpServerTransport>),
  #[cfg(feature = "matchbox")]
  WebRtc(Box<transport::webrtc::WebRtcServerTransport>),
}

/// A network server that manages client connections
///
/// Wraps renet's `RenetServer` with a transport abstraction that supports
/// both UDP (LAN) and WebRTC (internet with NAT traversal).
pub struct NetworkServer {
  server: RenetServer,
  transport: ServerTransportImpl,
  events: Vec<NetworkEvent>,
}

impl NetworkServer {
  /// Create and start a new server
  ///
  /// For UDP transport, binds to `0.0.0.0:{port}` to accept connections.
  /// For WebRTC transport, connects to the signaling server.
  ///
  /// # Errors
  ///
  /// Returns `NetworkError::InitError` if socket binding or transport
  /// creation fails.
  pub fn new(config: NetworkServerConfig) -> NetworkResult<Self> {
    let transport = match config.transport {
      TransportConfig::Udp => {
        let udp = transport::udp::UdpServerTransport::new(
          config.port,
          config.max_clients,
          config.protocol_id,
        )?;
        ServerTransportImpl::Udp(Box::new(udp))
      }
      #[cfg(feature = "matchbox")]
      TransportConfig::WebRtc(ref webrtc_config) => {
        let webrtc = transport::webrtc::WebRtcServerTransport::new(webrtc_config)?;
        ServerTransportImpl::WebRtc(Box::new(webrtc))
      }
    };

    let connection_config = connection_config();
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

    match &mut self.transport {
      ServerTransportImpl::Udp(udp) => {
        if let Err(e) = udp.update_with_server(dt, &mut self.server) {
          eprintln!("Network transport error: {}", e);
        }
        // Collect events from UDP transport
        self.events.extend(udp.drain_events());
      }
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => {
        if let Err(e) = webrtc.update(dt) {
          eprintln!("Network transport error: {}", e);
        }
        // Collect events from WebRTC transport
        self.events.extend(webrtc.drain_events());
      }
    }

    // For UDP, also collect renet server events
    if matches!(self.transport, ServerTransportImpl::Udp(_)) {
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
    }

    // Send outgoing packets
    match &mut self.transport {
      ServerTransportImpl::Udp(udp) => {
        udp.send_packets(&mut self.server);
      }
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(_) => {
        // WebRTC sends packets directly in its send methods
      }
    }
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
    match &mut self.transport {
      ServerTransportImpl::Udp(_) => {
        self
          .server
          .send_message(client_id, u8::from(channel), data.to_vec());
      }
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => {
        let channel_idx = webrtc_channel(channel);
        webrtc.send(client_id, channel_idx, data);
      }
    }
  }

  /// Broadcast a message to all connected clients
  pub fn broadcast(&mut self, channel: Channel, data: &[u8]) {
    match &mut self.transport {
      ServerTransportImpl::Udp(_) => {
        self
          .server
          .broadcast_message(u8::from(channel), data.to_vec());
      }
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => {
        let channel_idx = webrtc_channel(channel);
        webrtc.broadcast(channel_idx, data);
      }
    }
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
    match &mut self.transport {
      ServerTransportImpl::Udp(_) => self
        .server
        .receive_message(client_id, u8::from(channel))
        .map(|bytes| bytes.to_vec()),
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => {
        let channel_idx = webrtc_channel(channel);
        webrtc.receive(client_id, channel_idx)
      }
    }
  }

  /// Get an iterator over all connected client IDs
  pub fn clients(&self) -> impl Iterator<Item = ClientId> + '_ {
    match &self.transport {
      ServerTransportImpl::Udp(_) => {
        Box::new(self.server.clients_id_iter()) as Box<dyn Iterator<Item = ClientId>>
      }
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => {
        Box::new(webrtc.clients()) as Box<dyn Iterator<Item = ClientId>>
      }
    }
  }

  /// Disconnect a specific client
  pub fn disconnect(&mut self, client_id: ClientId) {
    match &self.transport {
      ServerTransportImpl::Udp(_) => {
        self.server.disconnect(client_id);
      }
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(_) => {
        // WebRTC doesn't have a direct disconnect method per-peer
        // The peer will be removed when they disconnect
      }
    }
  }

  /// Check if the server is running
  ///
  /// Returns `true` if the server was successfully created.
  pub fn is_running(&self) -> bool {
    match &self.transport {
      ServerTransportImpl::Udp(udp) => udp.is_ready(),
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => webrtc.is_ready(),
    }
  }

  /// Get the number of connected clients
  pub fn connected_count(&self) -> usize {
    match &self.transport {
      ServerTransportImpl::Udp(_) => self.server.connected_clients(),
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => webrtc.clients().count(),
    }
  }

  /// Whether the signaling server connection is still active (WebRTC only).
  ///
  /// Returns `true` for UDP transports (no signaling server).
  /// For WebRTC, returns `false` after the signaling server disconnects.
  /// Existing peer connections continue working, but no new peers can join.
  pub fn signaling_connected(&self) -> bool {
    match &self.transport {
      ServerTransportImpl::Udp(_) => true,
      #[cfg(feature = "matchbox")]
      ServerTransportImpl::WebRtc(webrtc) => webrtc.signaling_connected(),
    }
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
  /// Server address to connect to (for UDP transport)
  pub server_addr: Option<SocketAddr>,

  /// Protocol ID - must match the server (default: 0)
  pub protocol_id: u64,

  /// Transport configuration
  pub transport: TransportConfig,
}

impl NetworkClientConfig {
  /// Create a UDP client configuration.
  ///
  /// This is the simplest option for LAN games.
  pub fn udp(server_addr: SocketAddr, protocol_id: u64) -> Self {
    Self {
      server_addr: Some(server_addr),
      protocol_id,
      transport: TransportConfig::Udp,
    }
  }

  /// Create a WebRTC client configuration.
  ///
  /// Use this for internet games that need NAT traversal.
  #[cfg(feature = "matchbox")]
  pub fn webrtc(config: transport::WebRtcConfig) -> Self {
    Self {
      server_addr: None,
      protocol_id: 0,
      transport: TransportConfig::WebRtc(config),
    }
  }
}

/// Internal enum for client transport implementations.
enum ClientTransportImpl {
  Udp(Box<transport::udp::UdpClientTransport>),
  #[cfg(feature = "matchbox")]
  WebRtc(Box<transport::webrtc::WebRtcClientTransport>),
}

/// A network client that connects to a server
///
/// Supports both UDP (LAN) and WebRTC (internet with NAT traversal) transports.
pub struct NetworkClient {
  client: RenetClient,
  transport: ClientTransportImpl,
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
    let transport = match config.transport {
      TransportConfig::Udp => {
        let server_addr = config.server_addr.ok_or_else(|| {
          NetworkError::ConnectionError("UDP transport requires server_addr".to_string())
        })?;
        let udp = transport::udp::UdpClientTransport::new(server_addr, config.protocol_id)?;
        ClientTransportImpl::Udp(Box::new(udp))
      }
      #[cfg(feature = "matchbox")]
      TransportConfig::WebRtc(ref webrtc_config) => {
        let webrtc = transport::webrtc::WebRtcClientTransport::new(webrtc_config)?;
        ClientTransportImpl::WebRtc(Box::new(webrtc))
      }
    };

    let connection_config = connection_config();
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

    match &mut self.transport {
      ClientTransportImpl::Udp(udp) => {
        if let Err(e) = udp.update_with_client(dt, &mut self.client) {
          eprintln!("Network transport error: {}", e);
        }

        // Check for connection state changes
        let is_connected = self.client.is_connected();

        if is_connected && !self.was_connected {
          self.events.push(NetworkEvent::Connected);
        } else if !is_connected && self.was_connected {
          let reason = udp
            .disconnect_reason()
            .unwrap_or_else(|| "Unknown".to_string());
          self.events.push(NetworkEvent::Disconnected { reason });
        }

        self.was_connected = is_connected;

        // Send outgoing packets
        let _ = udp.send_packets(&mut self.client);
      }
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => {
        if let Err(e) = webrtc.update(dt) {
          eprintln!("Network transport error: {}", e);
        }

        // Check for connection state changes
        let is_connected = webrtc.is_connected();

        if is_connected && !self.was_connected {
          self.events.push(NetworkEvent::Connected);
        } else if !is_connected && self.was_connected {
          let reason = webrtc
            .disconnect_reason()
            .unwrap_or_else(|| "Unknown".to_string());
          self.events.push(NetworkEvent::Disconnected { reason });
        }

        self.was_connected = is_connected;
      }
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
    match &mut self.transport {
      ClientTransportImpl::Udp(_) => {
        self.client.send_message(u8::from(channel), data.to_vec());
      }
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => {
        let channel_idx = webrtc_channel(channel);
        webrtc.send(channel_idx, data);
      }
    }
  }

  /// Receive a message from the server
  ///
  /// Returns `None` if no message is available on the specified channel.
  /// Call repeatedly until `None` to drain all messages.
  pub fn receive(&mut self, channel: Channel) -> Option<Vec<u8>> {
    match &mut self.transport {
      ClientTransportImpl::Udp(_) => self
        .client
        .receive_message(u8::from(channel))
        .map(|bytes| bytes.to_vec()),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => {
        let channel_idx = webrtc_channel(channel);
        webrtc.receive(channel_idx)
      }
    }
  }

  /// Check if connected to the server
  pub fn is_connected(&self) -> bool {
    match &self.transport {
      ClientTransportImpl::Udp(_) => self.client.is_connected(),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => webrtc.is_connected(),
    }
  }

  /// Check if currently connecting to the server
  pub fn is_connecting(&self) -> bool {
    match &self.transport {
      ClientTransportImpl::Udp(_) => self.client.is_connecting(),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => webrtc.is_connecting(),
    }
  }

  /// Check if disconnected from the server
  pub fn is_disconnected(&self) -> bool {
    match &self.transport {
      ClientTransportImpl::Udp(_) => self.client.is_disconnected(),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => !webrtc.is_connected() && !webrtc.is_connecting(),
    }
  }

  /// Disconnect from the server
  pub fn disconnect(&mut self) {
    match &mut self.transport {
      ClientTransportImpl::Udp(udp) => udp.disconnect(),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => webrtc.disconnect(),
    }
  }

  /// Get the disconnect reason if disconnected
  pub fn disconnect_reason(&self) -> Option<String> {
    match &self.transport {
      ClientTransportImpl::Udp(udp) => udp.disconnect_reason(),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => webrtc.disconnect_reason(),
    }
  }

  /// Get the client's unique ID
  pub fn client_id(&self) -> ClientId {
    match &self.transport {
      ClientTransportImpl::Udp(udp) => udp.client_id(),
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => webrtc.client_id(),
    }
  }

  /// Whether the signaling server connection is still active (WebRTC only).
  ///
  /// Returns `true` for UDP transports (no signaling server).
  /// For WebRTC, returns `false` after the signaling server disconnects.
  /// The existing peer connection to the server continues working.
  pub fn signaling_connected(&self) -> bool {
    match &self.transport {
      ClientTransportImpl::Udp(_) => true,
      #[cfg(feature = "matchbox")]
      ClientTransportImpl::WebRtc(webrtc) => webrtc.signaling_connected(),
    }
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
/// ```ignore
/// use ryo2d::prelude::*;
/// use std::time::Duration;
///
/// let config = NetworkServerConfig {
///     port: 5000,
///     max_clients: 4,
///     protocol_id: 12345,
///     ..Default::default()
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
/// let clients: Vec<_> = server.all_clients().collect();
/// for client_id in clients {
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

/// The client side of a connection - the counterpart of [`ServerLike`], so code like
/// [`ReplicationManager::process_incoming`](replication::ReplicationManager::process_incoming)
/// works with a [`NetworkClient`] or a test double.
pub trait ClientLike {
  /// Send a message to the server.
  fn send(&mut self, channel: Channel, data: &[u8]);

  /// Receive a message from the server, or `None` when none is waiting.
  fn receive(&mut self, channel: Channel) -> Option<Vec<u8>>;
}

impl ClientLike for NetworkClient {
  fn send(&mut self, channel: Channel, data: &[u8]) {
    NetworkClient::send(self, channel, data);
  }

  fn receive(&mut self, channel: Channel) -> Option<Vec<u8>> {
    NetworkClient::receive(self, channel)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const ALL_CHANNELS: [Channel; 5] = [
    Channel::Unreliable,
    Channel::ReliableUnordered,
    Channel::ReliableOrdered,
    Channel::ReplicationReliable,
    Channel::ReplicationUnreliable,
  ];

  #[test]
  fn channel_ids_are_distinct_and_match_renet_defaults() {
    assert_eq!(
      u8::from(Channel::Unreliable),
      u8::from(DefaultChannel::Unreliable)
    );
    assert_eq!(
      u8::from(Channel::ReliableUnordered),
      u8::from(DefaultChannel::ReliableUnordered)
    );
    assert_eq!(
      u8::from(Channel::ReliableOrdered),
      u8::from(DefaultChannel::ReliableOrdered)
    );
    let ids: std::collections::HashSet<u8> = ALL_CHANNELS.into_iter().map(u8::from).collect();
    assert_eq!(ids.len(), ALL_CHANNELS.len());
  }

  #[test]
  fn connection_config_carries_the_replication_channels_both_ways() {
    let config = connection_config();
    for list in [
      &config.server_channels_config,
      &config.client_channels_config,
    ] {
      let mut ids: Vec<u8> = list.iter().map(|c| c.channel_id).collect();
      ids.sort_unstable();
      let mut expected: Vec<u8> = ALL_CHANNELS.into_iter().map(u8::from).collect();
      expected.sort_unstable();
      assert_eq!(ids, expected);
    }
  }

  #[test]
  fn server_config_default() {
    let config = NetworkServerConfig::default();
    assert_eq!(config.port, 5000);
    assert_eq!(config.max_clients, 16);
    assert!(matches!(config.transport, TransportConfig::Udp));
  }

  #[test]
  fn partial_server_configs_keep_the_documented_defaults() {
    let config = NetworkServerConfig {
      port: 7777,
      ..Default::default()
    };
    assert_eq!(config.max_clients, 16);

    let config = NetworkServerConfig {
      max_clients: 4,
      ..Default::default()
    };
    assert_eq!(config.port, 5000);
  }

  #[test]
  fn server_config_new_uses_defaults() {
    let config = NetworkServerConfig::new();
    assert_eq!(config.port, 5000);
    assert_eq!(config.max_clients, 16);
    assert_eq!(config.protocol_id, 0);
    assert!(matches!(config.transport, TransportConfig::Udp));
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
  fn transport_config_default_is_udp() {
    let config = TransportConfig::default();
    assert!(matches!(config, TransportConfig::Udp));
  }

  #[test]
  fn client_config_udp() {
    let addr: SocketAddr = "127.0.0.1:5000".parse().unwrap();
    let config = NetworkClientConfig::udp(addr, 12345);
    assert_eq!(config.server_addr, Some(addr));
    assert_eq!(config.protocol_id, 12345);
    assert!(matches!(config.transport, TransportConfig::Udp));
  }

  #[test]
  fn listen_server_host_id() {
    // Note: This test creates a real server, so it needs a free port
    let config = NetworkServerConfig {
      port: 19000,
      max_clients: 4,
      protocol_id: 12345,
      transport: TransportConfig::Udp,
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
      transport: TransportConfig::Udp,
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
      transport: TransportConfig::Udp,
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
      transport: TransportConfig::Udp,
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
      transport: TransportConfig::Udp,
    };

    if let Ok(server) = ListenServer::new(config) {
      assert!(server.is_running());
    }
  }

  #[cfg(feature = "matchbox")]
  #[test]
  fn webrtc_config_builder() {
    let config = WebRtcConfig::new("wss://signal.example.com", "room-123");
    assert_eq!(config.signaling_url, "wss://signal.example.com");
    assert_eq!(config.room_id, "room-123");
    assert!(!config.ice_servers.is_empty()); // Has default STUN servers

    let config = config.with_turn("turn:relay.example.com:3478", "user", "pass");
    assert!(
      config
        .ice_servers
        .iter()
        .any(|s| s.url.starts_with("turn:"))
    );
  }
}
