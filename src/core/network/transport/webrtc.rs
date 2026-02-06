//! WebRTC transport implementation using Matchbox.
//!
//! This module provides NAT traversal capabilities through WebRTC,
//! using STUN for hole punching and optional TURN for relay fallback.
//! Requires a signaling server for peer discovery.
//!
//! # Signaling Server
//!
//! You must run your own signaling server. Install and run `matchbox_server`:
//!
//! ```bash
//! cargo install matchbox_server
//! matchbox_server
//! ```
//!
//! This starts a server on `ws://localhost:3536`. For production, deploy
//! behind HTTPS (e.g., `wss://match.yourgame.com`).
//!
//! # Room-based Matchmaking
//!
//! The room ID in the URL path groups players together:
//! - `ws://localhost:3536/my-game` - All players join same room
//! - `ws://localhost:3536/my-game?next=2` - Match next 2 players, then next 2, etc.
//! - `ws://localhost:3536/my-game-v1.0-ranked?next=4` - Version/mode scoping
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // Local development
//! let config = WebRtcConfig::new("ws://localhost:3536", "my-game-room");
//!
//! // Production with HTTPS
//! let config = WebRtcConfig::new("wss://match.yourgame.com", "lobby-abc123");
//!
//! // With TURN relay fallback for strict NATs
//! let config = WebRtcConfig::new("wss://match.yourgame.com", "room-123")
//!     .with_turn("turn:relay.example.com:3478", "username", "password");
//! ```

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::time::Duration;

use matchbox_socket::{MultipleChannels, PeerId, PeerState, WebRtcSocket};
use renet::{ClientId, RenetClient, RenetServer};

use crate::core::error::{NetworkError, NetworkResult};
use crate::core::network::NetworkEvent;

use super::{ClientTransport, ServerTransport};

/// Configuration for WebRTC transport.
///
/// WebRTC enables NAT traversal through STUN hole punching, with
/// optional TURN relay fallback for strict NAT environments.
#[derive(Debug, Clone)]
pub struct WebRtcConfig {
  /// WebSocket URL of the signaling server.
  ///
  /// Example: `"wss://signaling.example.com"`
  pub signaling_url: String,

  /// Room identifier for peer discovery.
  ///
  /// Peers in the same room can discover and connect to each other.
  pub room_id: String,

  /// ICE servers for STUN/TURN.
  ///
  /// Defaults to Google's public STUN servers.
  pub ice_servers: Vec<IceServer>,
}

impl WebRtcConfig {
  /// Create a new WebRTC configuration.
  ///
  /// Uses default public STUN servers. For production, consider
  /// adding your own STUN/TURN servers.
  ///
  /// # Arguments
  ///
  /// * `signaling_url` - WebSocket URL of the signaling server
  /// * `room_id` - Room identifier for peer discovery
  ///
  /// # Example
  ///
  /// ```ignore
  /// let config = WebRtcConfig::new(
  ///     "wss://signaling.example.com",
  ///     "game-room-abc123",
  /// );
  /// ```
  pub fn new(signaling_url: impl Into<String>, room_id: impl Into<String>) -> Self {
    Self {
      signaling_url: signaling_url.into(),
      room_id: room_id.into(),
      ice_servers: vec![
        // Default public STUN servers
        IceServer::stun("stun:stun.l.google.com:19302"),
        IceServer::stun("stun:stun1.l.google.com:19302"),
      ],
    }
  }

  /// Add a TURN relay server for fallback connectivity.
  ///
  /// TURN servers relay traffic when direct peer-to-peer connections
  /// fail (e.g., symmetric NAT). This ensures connectivity but adds
  /// latency and bandwidth costs.
  ///
  /// # Arguments
  ///
  /// * `url` - TURN server URL (e.g., `"turn:relay.example.com:3478"`)
  /// * `username` - Authentication username
  /// * `credential` - Authentication password
  ///
  /// # Example
  ///
  /// ```ignore
  /// let config = WebRtcConfig::new("wss://signal.example.com", "room")
  ///     .with_turn("turn:relay.example.com:3478", "user", "pass");
  /// ```
  pub fn with_turn(mut self, url: &str, username: &str, credential: &str) -> Self {
    self
      .ice_servers
      .push(IceServer::turn(url, username, credential));
    self
  }

  /// Add a custom STUN server.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let config = WebRtcConfig::new("wss://signal.example.com", "room")
  ///     .with_stun("stun:mystun.example.com:3478");
  /// ```
  pub fn with_stun(mut self, url: &str) -> Self {
    self.ice_servers.push(IceServer::stun(url));
    self
  }

  /// Build the full signaling URL with room ID.
  fn build_signaling_url(&self) -> String {
    // Matchbox expects the room as part of the URL path
    if self.signaling_url.ends_with('/') {
      format!("{}{}", self.signaling_url, self.room_id)
    } else {
      format!("{}/{}", self.signaling_url, self.room_id)
    }
  }
}

/// ICE server configuration (STUN or TURN).
#[derive(Debug, Clone)]
pub struct IceServer {
  /// Server URL (e.g., `"stun:stun.example.com:3478"` or `"turn:turn.example.com:3478"`)
  pub url: String,

  /// Username for TURN authentication (None for STUN)
  pub username: Option<String>,

  /// Credential for TURN authentication (None for STUN)
  pub credential: Option<String>,
}

impl IceServer {
  /// Create a STUN server configuration.
  pub fn stun(url: &str) -> Self {
    Self {
      url: url.to_string(),
      username: None,
      credential: None,
    }
  }

  /// Create a TURN server configuration with credentials.
  pub fn turn(url: &str, username: &str, credential: &str) -> Self {
    Self {
      url: url.to_string(),
      username: Some(username.to_string()),
      credential: Some(credential.to_string()),
    }
  }
}

/// WebRTC server transport using Matchbox.
///
/// This transport acts as the "host" in a peer-to-peer WebRTC mesh.
/// It connects to a signaling server to discover peers and establishes
/// direct connections using STUN/TURN.
pub struct WebRtcServerTransport {
  socket: WebRtcSocket<MultipleChannels>,
  /// Maps Matchbox PeerIds to renet ClientIds
  peer_to_client: HashMap<PeerId, ClientId>,
  /// Maps renet ClientIds to Matchbox PeerIds
  client_to_peer: HashMap<ClientId, PeerId>,
  /// Next available client ID
  next_client_id: u64,
  /// Pending network events
  events: Vec<NetworkEvent>,
  /// Whether the socket is connected to the signaling server
  is_connected: bool,
}

impl WebRtcServerTransport {
  /// Create a new WebRTC server transport.
  ///
  /// Connects to the signaling server and waits for peers.
  pub fn new(config: &WebRtcConfig) -> NetworkResult<Self> {
    let url = config.build_signaling_url();

    // Create the WebRTC socket
    // Note: We use pollster::block_on since Matchbox is async but renet is sync.
    // Matchbox's build() can panic on connection failure, so we catch that.
    let result = panic::catch_unwind(|| {
      pollster::block_on(async {
        WebRtcSocket::builder(&url)
          .add_reliable_channel()
          .add_unreliable_channel()
          .build()
      })
    });

    let (socket, _loop_fut) = result.map_err(|e| {
      let msg = e
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .or_else(|| e.downcast_ref::<&str>().copied())
        .unwrap_or("unknown error");
      NetworkError::ConnectionError(format!(
        "Failed to connect to signaling server '{}': {}",
        url, msg
      ))
    })?;

    Ok(Self {
      socket,
      peer_to_client: HashMap::new(),
      client_to_peer: HashMap::new(),
      next_client_id: 1, // Start at 1, 0 is reserved for LOCAL_HOST_ID
      events: Vec::new(),
      is_connected: false,
    })
  }

  /// Poll the socket for new events.
  fn poll_socket(&mut self) -> NetworkResult<()> {
    // Matchbox can panic internally on signaling server failures,
    // so we catch panics and convert them to errors.
    let peers = panic::catch_unwind(AssertUnwindSafe(|| self.socket.update_peers()))
      .map_err(|e| {
        let msg = panic_message(&e);
        NetworkError::ConnectionError(format!("WebRTC signaling failed: {}", msg))
      })?;

    // Process peer state changes
    for (peer_id, state) in peers {
      match state {
        PeerState::Connected => {
          // Assign a new client ID
          let client_id: ClientId = self.next_client_id;
          self.next_client_id += 1;

          self.peer_to_client.insert(peer_id, client_id);
          self.client_to_peer.insert(client_id, peer_id);

          self
            .events
            .push(NetworkEvent::ClientConnected { client_id });
        }
        PeerState::Disconnected => {
          if let Some(client_id) = self.peer_to_client.remove(&peer_id) {
            self.client_to_peer.remove(&client_id);
            self.events.push(NetworkEvent::ClientDisconnected {
              client_id,
              reason: "Peer disconnected".to_string(),
            });
          }
        }
      }
    }

    // Mark as connected once we've successfully polled
    if !self.is_connected {
      self.is_connected = true;
    }

    Ok(())
  }

  /// Get the PeerId for a ClientId.
  #[allow(dead_code)]
  pub fn get_peer(&self, client_id: ClientId) -> Option<PeerId> {
    self.client_to_peer.get(&client_id).copied()
  }

  /// Get all connected client IDs.
  pub fn clients(&self) -> impl Iterator<Item = ClientId> + '_ {
    self.client_to_peer.keys().copied()
  }

  /// Send data to a specific peer on a channel.
  ///
  /// Channel 0 is reliable, channel 1 is unreliable.
  pub fn send(&mut self, client_id: ClientId, channel: usize, data: &[u8]) {
    if let Some(&peer_id) = self.client_to_peer.get(&client_id) {
      let packet = data.to_vec().into_boxed_slice();
      // Matchbox uses channel() which returns a mutable reference
      self.socket.channel_mut(channel).send(packet, peer_id);
    }
  }

  /// Receive data from a specific peer on a channel.
  pub fn receive(&mut self, client_id: ClientId, channel: usize) -> Option<Vec<u8>> {
    let peer_id = self.client_to_peer.get(&client_id)?;

    // Try to receive from the socket
    // Note: Matchbox doesn't filter by peer, so we need to check all messages
    for (from_peer, data) in self.socket.channel_mut(channel).receive() {
      if from_peer == *peer_id {
        return Some(data.to_vec());
      }
    }

    None
  }

  /// Broadcast data to all connected peers.
  pub fn broadcast(&mut self, channel: usize, data: &[u8]) {
    let packet = data.to_vec().into_boxed_slice();
    let peers: Vec<_> = self.peer_to_client.keys().copied().collect();

    for peer_id in peers {
      self
        .socket
        .channel_mut(channel)
        .send(packet.clone(), peer_id);
    }
  }
}

impl ServerTransport for WebRtcServerTransport {
  fn update(&mut self, _dt: Duration) -> NetworkResult<()> {
    self.poll_socket()
  }

  fn send_packets(&mut self, _server: &mut RenetServer) {
    // For WebRTC, we handle packets directly through the socket
    // This is called after renet processes messages, but we don't
    // need to do anything here since we send immediately in send()
  }

  fn drain_events(&mut self) -> Vec<NetworkEvent> {
    std::mem::take(&mut self.events)
  }

  fn is_ready(&self) -> bool {
    self.is_connected
  }
}

/// WebRTC client transport using Matchbox.
///
/// Connects to a signaling server and establishes a peer-to-peer
/// connection with the server (host).
pub struct WebRtcClientTransport {
  socket: WebRtcSocket<MultipleChannels>,
  /// The server's PeerId once connected
  server_peer: Option<PeerId>,
  /// Our client ID (assigned locally for now)
  client_id: ClientId,
  /// Connection state
  is_connected: bool,
  /// Whether we've been disconnected
  is_disconnected: bool,
  /// Disconnect reason
  disconnect_reason: Option<String>,
}

impl WebRtcClientTransport {
  /// Create a new WebRTC client transport.
  ///
  /// Connects to the signaling server to find the host.
  pub fn new(config: &WebRtcConfig) -> NetworkResult<Self> {
    let url = config.build_signaling_url();

    // Generate a random client ID
    let client_id: ClientId = rand_u64();

    // Create the WebRTC socket
    // Matchbox's build() can panic on connection failure, so we catch that.
    let result = panic::catch_unwind(|| {
      pollster::block_on(async {
        WebRtcSocket::builder(&url)
          .add_reliable_channel()
          .add_unreliable_channel()
          .build()
      })
    });

    let (socket, _loop_fut) = result.map_err(|e| {
      let msg = e
        .downcast_ref::<String>()
        .map(|s| s.as_str())
        .or_else(|| e.downcast_ref::<&str>().copied())
        .unwrap_or("unknown error");
      NetworkError::ConnectionError(format!(
        "Failed to connect to signaling server '{}': {}",
        url, msg
      ))
    })?;

    Ok(Self {
      socket,
      server_peer: None,
      client_id,
      is_connected: false,
      is_disconnected: false,
      disconnect_reason: None,
    })
  }

  /// Poll the socket for new events.
  fn poll_socket(&mut self) -> NetworkResult<()> {
    // Matchbox can panic internally on signaling server failures,
    // so we catch panics and convert them to errors.
    let peers = panic::catch_unwind(AssertUnwindSafe(|| self.socket.update_peers()))
      .map_err(|e| {
        let msg = panic_message(&e);
        self.is_disconnected = true;
        self.disconnect_reason = Some(format!("WebRTC signaling failed: {}", msg));
        NetworkError::ConnectionError(format!("WebRTC signaling failed: {}", msg))
      })?;

    for (peer_id, state) in peers {
      match state {
        PeerState::Connected => {
          // In a client-server model, the first peer we connect to is the server
          if self.server_peer.is_none() {
            self.server_peer = Some(peer_id);
            self.is_connected = true;
          }
        }
        PeerState::Disconnected => {
          if self.server_peer == Some(peer_id) {
            self.server_peer = None;
            self.is_connected = false;
            self.is_disconnected = true;
            self.disconnect_reason = Some("Server disconnected".to_string());
          }
        }
      }
    }

    Ok(())
  }

  /// Send data to the server on a channel.
  pub fn send(&mut self, channel: usize, data: &[u8]) {
    if let Some(server_peer) = self.server_peer {
      let packet = data.to_vec().into_boxed_slice();
      self.socket.channel_mut(channel).send(packet, server_peer);
    }
  }

  /// Receive data from the server on a channel.
  pub fn receive(&mut self, channel: usize) -> Option<Vec<u8>> {
    let server_peer = self.server_peer?;

    for (from_peer, data) in self.socket.channel_mut(channel).receive() {
      if from_peer == server_peer {
        return Some(data.to_vec());
      }
    }

    None
  }

  /// Check if connected to the server.
  pub fn is_connected(&self) -> bool {
    self.is_connected
  }

  /// Check if connecting to the server.
  pub fn is_connecting(&self) -> bool {
    !self.is_connected && !self.is_disconnected
  }
}

impl ClientTransport for WebRtcClientTransport {
  fn update(&mut self, _dt: Duration) -> NetworkResult<()> {
    self.poll_socket()
  }

  fn send_packets(&mut self, _client: &mut RenetClient) -> NetworkResult<()> {
    // For WebRTC, we handle packets directly through the socket
    Ok(())
  }

  fn disconnect(&mut self) {
    self.is_connected = false;
    self.is_disconnected = true;
    self.server_peer = None;
    // Socket will be dropped when transport is dropped
  }

  fn disconnect_reason(&self) -> Option<String> {
    self.disconnect_reason.clone()
  }

  fn client_id(&self) -> ClientId {
    self.client_id
  }

  fn is_ready(&self) -> bool {
    self.is_connected || self.is_connecting()
  }
}

/// Generate a random u64 using system randomness.
fn rand_u64() -> u64 {
  use std::collections::hash_map::RandomState;
  use std::hash::{BuildHasher, Hasher};
  use std::time::SystemTime;

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

/// Extract a message from a caught panic payload.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> &str {
  payload
    .downcast_ref::<String>()
    .map(|s| s.as_str())
    .or_else(|| payload.downcast_ref::<&str>().copied())
    .unwrap_or("signaling connection closed")
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  // =========================================================================
  // WebRtcConfig Tests
  // =========================================================================

  #[test]
  fn webrtc_config_new_creates_config() {
    let config = WebRtcConfig::new("ws://localhost:3536", "test-room");

    assert_eq!(config.signaling_url, "ws://localhost:3536");
    assert_eq!(config.room_id, "test-room");
  }

  #[test]
  fn webrtc_config_new_accepts_string() {
    let url = String::from("wss://signaling.example.com");
    let room = String::from("my-game-room");
    let config = WebRtcConfig::new(url, room);

    assert_eq!(config.signaling_url, "wss://signaling.example.com");
    assert_eq!(config.room_id, "my-game-room");
  }

  #[test]
  fn webrtc_config_has_default_stun_servers() {
    let config = WebRtcConfig::new("ws://localhost:3536", "room");

    // Should have at least one STUN server
    assert!(!config.ice_servers.is_empty());

    // Default servers should be STUN (no credentials)
    for server in &config.ice_servers {
      assert!(server.url.starts_with("stun:"));
      assert!(server.username.is_none());
      assert!(server.credential.is_none());
    }
  }

  #[test]
  fn webrtc_config_with_turn_adds_turn_server() {
    let config = WebRtcConfig::new("ws://localhost:3536", "room").with_turn(
      "turn:relay.example.com:3478",
      "user",
      "pass",
    );

    // Should have STUN servers + 1 TURN server
    let turn_servers: Vec<_> = config
      .ice_servers
      .iter()
      .filter(|s| s.url.starts_with("turn:"))
      .collect();

    assert_eq!(turn_servers.len(), 1);

    let turn = turn_servers[0];
    assert_eq!(turn.url, "turn:relay.example.com:3478");
    assert_eq!(turn.username.as_deref(), Some("user"));
    assert_eq!(turn.credential.as_deref(), Some("pass"));
  }

  #[test]
  fn webrtc_config_with_stun_adds_stun_server() {
    let config =
      WebRtcConfig::new("ws://localhost:3536", "room").with_stun("stun:mystun.example.com:3478");

    // Should have default STUN servers + 1 custom STUN server
    let custom_stun = config
      .ice_servers
      .iter()
      .find(|s| s.url == "stun:mystun.example.com:3478");

    assert!(custom_stun.is_some());
    let stun = custom_stun.unwrap();
    assert!(stun.username.is_none());
    assert!(stun.credential.is_none());
  }

  #[test]
  fn webrtc_config_builds_signaling_url_with_room() {
    let config = WebRtcConfig::new("ws://localhost:3536", "game-room");
    let url = config.build_signaling_url();

    assert_eq!(url, "ws://localhost:3536/game-room");
  }

  #[test]
  fn webrtc_config_builds_signaling_url_handles_trailing_slash() {
    let config = WebRtcConfig::new("ws://localhost:3536/", "game-room");
    let url = config.build_signaling_url();

    assert_eq!(url, "ws://localhost:3536/game-room");
  }

  #[test]
  fn webrtc_config_builder_chain_works() {
    let config = WebRtcConfig::new("wss://signal.example.com", "lobby")
      .with_stun("stun:custom.stun.com:3478")
      .with_turn("turn:relay.example.com:3478", "user1", "pass1")
      .with_turn("turn:backup.example.com:3478", "user2", "pass2");

    // Count TURN servers
    let turn_count = config
      .ice_servers
      .iter()
      .filter(|s| s.url.starts_with("turn:"))
      .count();
    assert_eq!(turn_count, 2);

    // Verify custom STUN exists
    let has_custom_stun = config
      .ice_servers
      .iter()
      .any(|s| s.url == "stun:custom.stun.com:3478");
    assert!(has_custom_stun);
  }

  #[test]
  fn webrtc_config_is_clone() {
    let config = WebRtcConfig::new("ws://localhost:3536", "room").with_turn(
      "turn:relay.example.com:3478",
      "user",
      "pass",
    );

    let cloned = config.clone();

    assert_eq!(cloned.signaling_url, config.signaling_url);
    assert_eq!(cloned.room_id, config.room_id);
    assert_eq!(cloned.ice_servers.len(), config.ice_servers.len());
  }

  #[test]
  fn webrtc_config_is_debug() {
    let config = WebRtcConfig::new("ws://localhost:3536", "room");
    let debug = format!("{:?}", config);

    assert!(debug.contains("WebRtcConfig"));
    assert!(debug.contains("localhost:3536"));
  }

  // =========================================================================
  // IceServer Tests
  // =========================================================================

  #[test]
  fn ice_server_stun_creates_server_without_credentials() {
    let server = IceServer::stun("stun:stun.example.com:3478");

    assert_eq!(server.url, "stun:stun.example.com:3478");
    assert!(server.username.is_none());
    assert!(server.credential.is_none());
  }

  #[test]
  fn ice_server_turn_creates_server_with_credentials() {
    let server = IceServer::turn("turn:turn.example.com:3478", "myuser", "mypass");

    assert_eq!(server.url, "turn:turn.example.com:3478");
    assert_eq!(server.username.as_deref(), Some("myuser"));
    assert_eq!(server.credential.as_deref(), Some("mypass"));
  }

  #[test]
  fn ice_server_is_clone() {
    let server = IceServer::turn("turn:turn.example.com:3478", "user", "pass");
    let cloned = server.clone();

    assert_eq!(cloned.url, server.url);
    assert_eq!(cloned.username, server.username);
    assert_eq!(cloned.credential, server.credential);
  }

  #[test]
  fn ice_server_is_debug() {
    let server = IceServer::stun("stun:stun.example.com:3478");
    let debug = format!("{:?}", server);

    assert!(debug.contains("IceServer"));
    assert!(debug.contains("stun.example.com"));
  }

  // =========================================================================
  // WebRtcClientTransport Tests (unit tests that don't need network)
  // =========================================================================

  #[test]
  fn webrtc_client_transport_is_connecting_initially() {
    // We can't easily test without a signaling server, but we can verify
    // the state logic methods
    let transport = WebRtcClientTransport {
      socket: create_mock_socket(),
      server_peer: None,
      client_id: 12345,
      is_connected: false,
      is_disconnected: false,
      disconnect_reason: None,
    };

    assert!(transport.is_connecting());
    assert!(!transport.is_connected());
  }

  #[test]
  fn webrtc_client_transport_is_ready_when_connecting() {
    let transport = WebRtcClientTransport {
      socket: create_mock_socket(),
      server_peer: None,
      client_id: 12345,
      is_connected: false,
      is_disconnected: false,
      disconnect_reason: None,
    };

    // Should be ready when connecting (not yet connected or disconnected)
    assert!(transport.is_ready());
  }

  #[test]
  fn webrtc_client_transport_client_id_returns_assigned_id() {
    let transport = WebRtcClientTransport {
      socket: create_mock_socket(),
      server_peer: None,
      client_id: 99999,
      is_connected: false,
      is_disconnected: false,
      disconnect_reason: None,
    };

    assert_eq!(transport.client_id(), 99999);
  }

  // =========================================================================
  // WebRtcServerTransport Tests (unit tests that don't need network)
  // =========================================================================

  #[test]
  fn webrtc_server_transport_starts_with_no_clients() {
    let transport = WebRtcServerTransport {
      socket: create_mock_socket(),
      peer_to_client: HashMap::new(),
      client_to_peer: HashMap::new(),
      next_client_id: 1,
      events: Vec::new(),
      is_connected: false,
    };

    let clients: Vec<_> = transport.clients().collect();
    assert!(clients.is_empty());
  }

  #[test]
  fn webrtc_server_transport_events_starts_empty() {
    let mut transport = WebRtcServerTransport {
      socket: create_mock_socket(),
      peer_to_client: HashMap::new(),
      client_to_peer: HashMap::new(),
      next_client_id: 1,
      events: Vec::new(),
      is_connected: false,
    };

    let events = transport.drain_events();
    assert!(events.is_empty());
  }

  #[test]
  fn webrtc_server_transport_not_ready_initially() {
    let transport = WebRtcServerTransport {
      socket: create_mock_socket(),
      peer_to_client: HashMap::new(),
      client_to_peer: HashMap::new(),
      next_client_id: 1,
      events: Vec::new(),
      is_connected: false,
    };

    assert!(!transport.is_ready());
  }

  // =========================================================================
  // Helper Functions
  // =========================================================================

  /// Create a mock WebRTC socket for testing.
  /// This connects to a non-existent server but allows us to test struct creation.
  fn create_mock_socket() -> WebRtcSocket<MultipleChannels> {
    // Use a URL that won't connect but allows socket creation
    let (socket, _) = pollster::block_on(async {
      WebRtcSocket::builder("ws://127.0.0.1:1")
        .add_reliable_channel()
        .add_unreliable_channel()
        .build()
    });
    socket
  }
}
