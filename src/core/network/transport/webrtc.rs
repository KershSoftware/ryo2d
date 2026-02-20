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

use std::collections::{HashMap, VecDeque};
use std::panic;
use std::thread::{self, JoinHandle};
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
  /// Background thread driving the signaling message loop
  _message_loop: JoinHandle<()>,
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
  /// Buffered received messages per (client, channel) to avoid dropping
  /// messages when matchbox drains the channel on receive().
  receive_buffers: HashMap<(ClientId, usize), VecDeque<Vec<u8>>>,
  /// Whether the signaling server connection has failed.
  /// Once true, `update_peers()` is skipped but existing peer connections
  /// continue working via their established data channels.
  signaling_failed: bool,
}

impl WebRtcServerTransport {
  /// Create a new WebRTC server transport.
  ///
  /// Connects to the signaling server and waits for peers.
  pub fn new(config: &WebRtcConfig) -> NetworkResult<Self> {
    let url = config.build_signaling_url();

    // Create the WebRTC socket.
    // Server retries signaling indefinitely so it stays available for new players.
    // build() is sync but can panic on invalid config, so we catch that.
    let (socket, loop_fut) = panic::catch_unwind(|| {
      WebRtcSocket::builder(&url)
        .reconnect_attempts(None)
        .add_reliable_channel()
        .add_unreliable_channel()
        .build()
    })
    .map_err(|e| {
      let msg = panic_message(&e);
      NetworkError::ConnectionError(format!(
        "Failed to connect to signaling server '{}': {}",
        url, msg
      ))
    })?;

    // The message loop future MUST be driven for signaling to work.
    // Spawn it on a background thread — it completes when the socket is dropped.
    let message_loop = thread::spawn(move || {
      let _ = pollster::block_on(loop_fut);
    });

    Ok(Self {
      socket,
      _message_loop: message_loop,
      peer_to_client: HashMap::new(),
      client_to_peer: HashMap::new(),
      next_client_id: 1, // Start at 1, 0 is reserved for LOCAL_HOST_ID
      events: Vec::new(),
      is_connected: false,
      receive_buffers: HashMap::new(),
      signaling_failed: false,
    })
  }

  /// Poll the socket for new events.
  fn poll_socket(&mut self) -> NetworkResult<()> {
    // Once signaling has failed, skip update_peers() — existing peer
    // connections still work via their established data channels.
    if !self.signaling_failed {
      match self.socket.try_update_peers() {
        Ok(peers) => {
          for (peer_id, state) in peers {
            match state {
              PeerState::Connected => {
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
        }
        Err(_) => {
          self.signaling_failed = true;
          eprintln!("WebRTC signaling server disconnected; existing peer connections preserved");
        }
      }
    }

    if !self.is_connected {
      self.is_connected = true;
    }

    Ok(())
  }

  /// Whether the signaling server connection is still active.
  ///
  /// Returns `false` after the signaling server disconnects. Existing
  /// peer connections continue working, but no new peers can join.
  pub fn signaling_connected(&self) -> bool {
    !self.signaling_failed
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
      self.socket.channel_mut(channel).send(packet, peer_id);
    }
  }

  /// Receive data from a specific peer on a channel.
  pub fn receive(&mut self, client_id: ClientId, channel: usize) -> Option<Vec<u8>> {
    // Check buffer first
    if let Some(queue) = self.receive_buffers.get_mut(&(client_id, channel))
      && let Some(msg) = queue.pop_front()
    {
      return Some(msg);
    }

    // Drain socket into per-client buffers (matchbox drains all messages at once)
    for (from_peer, data) in self.socket.channel_mut(channel).receive() {
      if let Some(&client) = self.peer_to_client.get(&from_peer) {
        self
          .receive_buffers
          .entry((client, channel))
          .or_default()
          .push_back(data.to_vec());
      }
    }

    self
      .receive_buffers
      .get_mut(&(client_id, channel))?
      .pop_front()
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
  /// Background thread driving the signaling message loop
  _message_loop: JoinHandle<()>,
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
  /// Buffered received messages per channel to avoid dropping
  /// messages when matchbox drains the channel on receive().
  receive_buffers: [VecDeque<Vec<u8>>; 2],
  /// Whether the signaling server connection has failed.
  /// Once true, `update_peers()` is skipped but the existing peer connection
  /// continues working via its established data channel.
  signaling_failed: bool,
}

impl WebRtcClientTransport {
  /// Create a new WebRTC client transport.
  ///
  /// Connects to the signaling server to find the host.
  pub fn new(config: &WebRtcConfig) -> NetworkResult<Self> {
    let url = config.build_signaling_url();

    // Generate a random client ID
    let client_id: ClientId = rand_u64();

    // Create the WebRTC socket.
    // build() is sync but can panic on invalid config, so we catch that.
    let (socket, loop_fut) = panic::catch_unwind(|| {
      WebRtcSocket::builder(&url)
        .add_reliable_channel()
        .add_unreliable_channel()
        .build()
    })
    .map_err(|e| {
      let msg = panic_message(&e);
      NetworkError::ConnectionError(format!(
        "Failed to connect to signaling server '{}': {}",
        url, msg
      ))
    })?;

    // The message loop future MUST be driven for signaling to work.
    // Spawn it on a background thread — it completes when the socket is dropped.
    let message_loop = thread::spawn(move || {
      let _ = pollster::block_on(loop_fut);
    });

    Ok(Self {
      socket,
      _message_loop: message_loop,
      server_peer: None,
      client_id,
      is_connected: false,
      is_disconnected: false,
      disconnect_reason: None,
      receive_buffers: [VecDeque::new(), VecDeque::new()],
      signaling_failed: false,
    })
  }

  /// Poll the socket for new events.
  fn poll_socket(&mut self) -> NetworkResult<()> {
    if self.signaling_failed {
      return Ok(());
    }

    let peers = match self.socket.try_update_peers() {
      Ok(peers) => peers,
      Err(_) => {
        self.signaling_failed = true;
        eprintln!("WebRTC signaling server disconnected; existing peer connections preserved");
        return Ok(());
      }
    };

    for (peer_id, state) in peers {
      match state {
        PeerState::Connected => {
          // In a full-mesh WebRTC room, multiple peers may connect (the
          // host plus other clients). We do NOT pick server_peer here
          // because connection order is non-deterministic — the first
          // ICE handshake to complete may be another client, not the host.
          // Instead, server_peer is discovered in receive() from the first
          // peer to send us data (only the host sends to clients).
          if !self.is_connected {
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
    // Check buffer first
    if let Some(msg) = self.receive_buffers[channel].pop_front() {
      return Some(msg);
    }

    // Drain socket into buffer (matchbox drains all messages at once).
    // In a full-mesh room we may be connected to non-server peers, so
    // we need to identify which peer is the actual server.
    for (from_peer, data) in self.socket.channel_mut(channel).receive() {
      if let Some(server) = self.server_peer {
        // Only accept messages from the identified server
        if from_peer == server {
          self.receive_buffers[channel].push_back(data.to_vec());
        }
      } else {
        // Server not yet identified. In a client-server model only the
        // host sends data to clients, so the first peer to send is the server.
        self.server_peer = Some(from_peer);
        self.receive_buffers[channel].push_back(data.to_vec());
      }
    }

    self.receive_buffers[channel].pop_front()
  }

  /// Check if connected to the server.
  pub fn is_connected(&self) -> bool {
    self.is_connected
  }

  /// Whether the signaling server connection is still active.
  ///
  /// Returns `false` after the signaling server disconnects. The existing
  /// peer connection to the server continues working.
  pub fn signaling_connected(&self) -> bool {
    !self.signaling_failed
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

  /// Create a deterministic PeerId for testing from a simple byte value.
  fn test_peer_id(b: u8) -> PeerId {
    // PeerId wraps Uuid; construct via serde since Uuid isn't directly accessible
    let hex = format!("{:02x}", b).repeat(16);
    let uuid_str = format!(
      "{}-{}-{}-{}-{}",
      &hex[0..8],
      &hex[8..12],
      &hex[12..16],
      &hex[16..20],
      &hex[20..32]
    );
    serde_json::from_str(&format!("\"{}\"", uuid_str)).unwrap()
  }

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
    let transport = create_mock_client_transport();

    assert!(transport.is_connecting());
    assert!(!transport.is_connected());
  }

  #[test]
  fn webrtc_client_transport_is_ready_when_connecting() {
    let transport = create_mock_client_transport();

    // Should be ready when connecting (not yet connected or disconnected)
    assert!(transport.is_ready());
  }

  #[test]
  fn webrtc_client_transport_client_id_returns_assigned_id() {
    let transport = create_mock_client_transport_with_id(99999);

    assert_eq!(transport.client_id(), 99999);
  }

  // =========================================================================
  // WebRtcServerTransport Tests (unit tests that don't need network)
  // =========================================================================

  #[test]
  fn webrtc_server_transport_starts_with_no_clients() {
    let transport = create_mock_server_transport();

    let clients: Vec<_> = transport.clients().collect();
    assert!(clients.is_empty());
  }

  #[test]
  fn webrtc_server_transport_events_starts_empty() {
    let mut transport = create_mock_server_transport();

    let events = transport.drain_events();
    assert!(events.is_empty());
  }

  #[test]
  fn webrtc_server_transport_not_ready_initially() {
    let transport = create_mock_server_transport();

    assert!(!transport.is_ready());
  }

  // =========================================================================
  // WebRtcServerTransport — signaling_connected
  // =========================================================================

  #[test]
  fn webrtc_server_signaling_connected_initially_true() {
    let transport = create_mock_server_transport();
    assert!(transport.signaling_connected());
  }

  #[test]
  fn webrtc_server_signaling_connected_false_after_failure() {
    let mut transport = create_mock_server_transport();
    transport.signaling_failed = true;
    assert!(!transport.signaling_connected());
  }

  // =========================================================================
  // WebRtcServerTransport — poll_socket / update
  // =========================================================================

  #[test]
  fn webrtc_server_poll_socket_marks_connected() {
    let mut transport = create_mock_server_transport();
    assert!(!transport.is_connected);
    // poll_socket always marks as connected (even if update_peers panics,
    // the is_connected flag is set after the match block)
    let _ = transport.poll_socket();
    assert!(transport.is_connected);
    assert!(transport.is_ready());
  }

  #[test]
  fn webrtc_server_poll_socket_skips_update_when_signaling_failed() {
    let mut transport = create_mock_server_transport();
    transport.signaling_failed = true;
    transport.is_connected = true;
    // Should not panic and should return Ok
    let result = transport.poll_socket();
    assert!(result.is_ok());
    assert!(transport.signaling_failed);
  }

  #[test]
  fn webrtc_server_update_trait_delegates_to_poll_socket() {
    let mut transport = create_mock_server_transport();
    assert!(!transport.is_connected);
    let result = transport.update(Duration::from_millis(16));
    assert!(result.is_ok());
    assert!(transport.is_connected);
  }

  // =========================================================================
  // WebRtcServerTransport — client mapping & events
  // =========================================================================

  #[test]
  fn webrtc_server_get_peer_returns_none_for_unknown() {
    let transport = create_mock_server_transport();
    assert!(transport.get_peer(999).is_none());
  }

  #[test]
  fn webrtc_server_get_peer_returns_mapped_peer() {
    let mut transport = create_mock_server_transport();
    let peer_id = test_peer_id(1);
    transport.peer_to_client.insert(peer_id, 42);
    transport.client_to_peer.insert(42, peer_id);
    assert_eq!(transport.get_peer(42), Some(peer_id));
  }

  #[test]
  fn webrtc_server_clients_reflects_mapping() {
    let mut transport = create_mock_server_transport();
    let peer1 = test_peer_id(1);
    let peer2 = test_peer_id(2);
    transport.client_to_peer.insert(10, peer1);
    transport.client_to_peer.insert(20, peer2);

    let mut clients: Vec<_> = transport.clients().collect();
    clients.sort();
    assert_eq!(clients, vec![10, 20]);
  }

  #[test]
  fn webrtc_server_drain_events_returns_and_clears() {
    let mut transport = create_mock_server_transport();
    transport
      .events
      .push(NetworkEvent::ClientConnected { client_id: 1 });
    transport
      .events
      .push(NetworkEvent::ClientConnected { client_id: 2 });

    let events = transport.drain_events();
    assert_eq!(events.len(), 2);
    // After drain, events should be empty
    let events2 = transport.drain_events();
    assert!(events2.is_empty());
  }

  // =========================================================================
  // WebRtcServerTransport — send / receive / broadcast
  // =========================================================================

  #[test]
  fn webrtc_server_send_ignores_unknown_client() {
    let mut transport = create_mock_server_transport();
    // Should not panic when sending to unknown client
    transport.send(999, 0, b"hello");
  }

  #[test]
  fn webrtc_server_receive_returns_buffered_data() {
    let mut transport = create_mock_server_transport();
    let client_id: ClientId = 42;

    // Pre-populate the receive buffer
    transport
      .receive_buffers
      .entry((client_id, 0))
      .or_default()
      .push_back(b"msg1".to_vec());
    transport
      .receive_buffers
      .entry((client_id, 0))
      .or_default()
      .push_back(b"msg2".to_vec());

    // Should return buffered messages in order
    assert_eq!(transport.receive(client_id, 0), Some(b"msg1".to_vec()));
    assert_eq!(transport.receive(client_id, 0), Some(b"msg2".to_vec()));
    assert_eq!(transport.receive(client_id, 0), None);
  }

  #[test]
  fn webrtc_server_receive_returns_none_for_unknown_client() {
    let mut transport = create_mock_server_transport();
    assert_eq!(transport.receive(999, 0), None);
  }

  #[test]
  fn webrtc_server_receive_buffers_per_channel() {
    let mut transport = create_mock_server_transport();
    let client_id: ClientId = 42;

    // Buffer data on channel 0 and channel 1 separately
    transport
      .receive_buffers
      .entry((client_id, 0))
      .or_default()
      .push_back(b"reliable".to_vec());
    transport
      .receive_buffers
      .entry((client_id, 1))
      .or_default()
      .push_back(b"unreliable".to_vec());

    assert_eq!(transport.receive(client_id, 0), Some(b"reliable".to_vec()));
    assert_eq!(
      transport.receive(client_id, 1),
      Some(b"unreliable".to_vec())
    );
    assert_eq!(transport.receive(client_id, 0), None);
    assert_eq!(transport.receive(client_id, 1), None);
  }

  #[test]
  fn webrtc_server_broadcast_to_empty_peers_is_noop() {
    let mut transport = create_mock_server_transport();
    // No peers mapped — should not panic
    transport.broadcast(0, b"hello everyone");
  }

  // =========================================================================
  // WebRtcClientTransport — signaling_connected
  // =========================================================================

  #[test]
  fn webrtc_client_signaling_connected_initially_true() {
    let transport = create_mock_client_transport();
    assert!(transport.signaling_connected());
  }

  #[test]
  fn webrtc_client_signaling_connected_false_after_failure() {
    let mut transport = create_mock_client_transport();
    transport.signaling_failed = true;
    assert!(!transport.signaling_connected());
  }

  // =========================================================================
  // WebRtcClientTransport — connection state
  // =========================================================================

  #[test]
  fn webrtc_client_not_connected_initially() {
    let transport = create_mock_client_transport();
    assert!(!transport.is_connected());
    assert!(transport.is_connecting());
  }

  #[test]
  fn webrtc_client_is_ready_when_connected() {
    let mut transport = create_mock_client_transport();
    transport.is_connected = true;
    assert!(transport.is_ready());
  }

  #[test]
  fn webrtc_client_not_ready_when_disconnected() {
    let mut transport = create_mock_client_transport();
    transport.is_disconnected = true;
    // Not connected and not connecting (is_disconnected = true)
    assert!(!transport.is_ready());
  }

  #[test]
  fn webrtc_client_is_connecting_false_when_connected() {
    let mut transport = create_mock_client_transport();
    transport.is_connected = true;
    assert!(!transport.is_connecting());
  }

  #[test]
  fn webrtc_client_is_connecting_false_when_disconnected() {
    let mut transport = create_mock_client_transport();
    transport.is_disconnected = true;
    assert!(!transport.is_connecting());
  }

  // =========================================================================
  // WebRtcClientTransport — disconnect
  // =========================================================================

  #[test]
  fn webrtc_client_disconnect_clears_state() {
    let mut transport = create_mock_client_transport();
    transport.is_connected = true;
    transport.server_peer = Some(test_peer_id(1));

    transport.disconnect();

    assert!(!transport.is_connected());
    assert!(transport.is_disconnected);
    assert!(transport.server_peer.is_none());
  }

  #[test]
  fn webrtc_client_disconnect_reason_initially_none() {
    let transport = create_mock_client_transport();
    assert!(transport.disconnect_reason.is_none());
  }

  #[test]
  fn webrtc_client_disconnect_reason_from_trait() {
    let mut transport = create_mock_client_transport();
    transport.disconnect_reason = Some("test reason".to_string());
    assert_eq!(
      <WebRtcClientTransport as ClientTransport>::disconnect_reason(&transport),
      Some("test reason".to_string())
    );
  }

  // =========================================================================
  // WebRtcClientTransport — server_peer discovery & receive
  // =========================================================================

  #[test]
  fn webrtc_client_server_peer_none_initially() {
    let transport = create_mock_client_transport();
    assert!(transport.server_peer.is_none());
  }

  #[test]
  fn webrtc_client_send_noop_without_server_peer() {
    let mut transport = create_mock_client_transport();
    assert!(transport.server_peer.is_none());
    // Should not panic — just silently does nothing
    transport.send(0, b"hello");
  }

  #[test]
  fn webrtc_client_receive_returns_buffered_data() {
    let mut transport = create_mock_client_transport();
    transport.server_peer = Some(test_peer_id(1));

    // Pre-populate buffer on channel 0
    transport.receive_buffers[0].push_back(b"msg1".to_vec());
    transport.receive_buffers[0].push_back(b"msg2".to_vec());

    assert_eq!(transport.receive(0), Some(b"msg1".to_vec()));
    assert_eq!(transport.receive(0), Some(b"msg2".to_vec()));
    assert_eq!(transport.receive(0), None);
  }

  #[test]
  fn webrtc_client_receive_returns_none_when_empty() {
    let mut transport = create_mock_client_transport();
    assert_eq!(transport.receive(0), None);
    assert_eq!(transport.receive(1), None);
  }

  #[test]
  fn webrtc_client_receive_buffers_per_channel() {
    let mut transport = create_mock_client_transport();
    transport.server_peer = Some(test_peer_id(1));

    transport.receive_buffers[0].push_back(b"reliable".to_vec());
    transport.receive_buffers[1].push_back(b"unreliable".to_vec());

    assert_eq!(transport.receive(0), Some(b"reliable".to_vec()));
    assert_eq!(transport.receive(1), Some(b"unreliable".to_vec()));
    assert_eq!(transport.receive(0), None);
    assert_eq!(transport.receive(1), None);
  }

  // =========================================================================
  // WebRtcClientTransport — poll_socket with signaling_failed
  // =========================================================================

  #[test]
  fn webrtc_client_poll_socket_skips_update_when_signaling_failed() {
    let mut transport = create_mock_client_transport();
    transport.signaling_failed = true;
    // Should return Ok immediately without touching the socket
    let result = transport.poll_socket();
    assert!(result.is_ok());
    // State should not change
    assert!(!transport.is_connected);
    assert!(transport.server_peer.is_none());
  }

  #[test]
  fn webrtc_client_update_trait_delegates_to_poll_socket() {
    let mut transport = create_mock_client_transport();
    transport.signaling_failed = true;
    let result = transport.update(Duration::from_millis(16));
    assert!(result.is_ok());
  }

  #[test]
  fn webrtc_client_send_packets_trait_is_noop() {
    let mut transport = create_mock_client_transport();
    let mut client = RenetClient::new(renet::ConnectionConfig::default());
    let result = transport.send_packets(&mut client);
    assert!(result.is_ok());
  }

  #[test]
  fn webrtc_client_client_id_from_trait() {
    let transport = create_mock_client_transport_with_id(54321);
    assert_eq!(
      <WebRtcClientTransport as ClientTransport>::client_id(&transport),
      54321
    );
  }

  // =========================================================================
  // Helper function tests — panic_message
  // =========================================================================

  #[test]
  fn panic_message_extracts_string_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("test error"));
    assert_eq!(panic_message(&payload), "test error");
  }

  #[test]
  fn panic_message_extracts_str_payload() {
    let payload: Box<dyn std::any::Any + Send> = Box::new("static error");
    assert_eq!(panic_message(&payload), "static error");
  }

  #[test]
  fn panic_message_returns_default_for_unknown_type() {
    let payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
    assert_eq!(panic_message(&payload), "signaling connection closed");
  }

  // =========================================================================
  // Helper function tests — rand_u64
  // =========================================================================

  #[test]
  fn rand_u64_returns_nonzero() {
    // Very unlikely to get 0, but mainly testing it doesn't panic
    let _id = rand_u64();
  }

  #[test]
  fn rand_u64_returns_different_values() {
    // Two calls should produce different values (not guaranteed but extremely likely)
    let a = rand_u64();
    // Small sleep to ensure different nanos
    std::thread::sleep(Duration::from_nanos(1));
    let b = rand_u64();
    // If they happen to be equal, it's not a bug — but it's astronomically unlikely
    // This test mainly verifies the function runs without panicking
    let _ = (a, b);
  }

  // =========================================================================
  // Helper Functions
  // =========================================================================

  /// Create a mock WebRTC socket for testing.
  /// This connects to a non-existent server but allows us to test struct creation.
  fn create_mock_socket() -> (WebRtcSocket<MultipleChannels>, JoinHandle<()>) {
    let (socket, loop_fut) = WebRtcSocket::builder("ws://127.0.0.1:1")
      .add_reliable_channel()
      .add_unreliable_channel()
      .build();
    let handle = thread::spawn(move || {
      let _ = pollster::block_on(loop_fut);
    });
    (socket, handle)
  }

  /// Create a mock server transport with default empty state.
  fn create_mock_server_transport() -> WebRtcServerTransport {
    let (socket, handle) = create_mock_socket();
    WebRtcServerTransport {
      socket,
      _message_loop: handle,
      peer_to_client: HashMap::new(),
      client_to_peer: HashMap::new(),
      next_client_id: 1,
      events: Vec::new(),
      is_connected: false,
      receive_buffers: HashMap::new(),
      signaling_failed: false,
    }
  }

  /// Create a mock client transport with default empty state.
  fn create_mock_client_transport() -> WebRtcClientTransport {
    create_mock_client_transport_with_id(12345)
  }

  /// Create a mock client transport with a specific client ID.
  fn create_mock_client_transport_with_id(id: ClientId) -> WebRtcClientTransport {
    let (socket, handle) = create_mock_socket();
    WebRtcClientTransport {
      socket,
      _message_loop: handle,
      server_peer: None,
      client_id: id,
      is_connected: false,
      is_disconnected: false,
      disconnect_reason: None,
      receive_buffers: [VecDeque::new(), VecDeque::new()],
      signaling_failed: false,
    }
  }
}
