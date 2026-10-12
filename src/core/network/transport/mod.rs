//! Transport abstraction layer for network communication.
//!
//! This module provides a unified interface for different network transports,
//! allowing games to switch between UDP (for LAN) and WebRTC (for internet
//! with NAT traversal) with minimal code changes.
//!
//! # Transport Types
//!
//! - **UDP** (default): Direct UDP sockets, lowest latency, best for LAN games
//! - **WebRTC** (requires `matchbox` feature): NAT hole punching via STUN,
//!   with optional TURN relay fallback for strict NATs
//!
//! # Example
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! // LAN game - uses UDP (default)
//! let config = NetworkServerConfig::default();
//!
//! // Internet game - uses WebRTC for NAT traversal
//! // Run signaling server first: cargo install matchbox_server && matchbox_server
//! #[cfg(feature = "matchbox")]
//! let config = NetworkServerConfig {
//!     transport: TransportConfig::WebRtc(WebRtcConfig::new(
//!         "ws://localhost:3536",  // Local dev, use wss:// in production
//!         "my-game-room",
//!     )),
//!     ..Default::default()
//! };
//! ```

pub(crate) mod udp;

#[cfg(feature = "matchbox")]
pub(crate) mod webrtc;

use std::time::Duration;

use renet::ClientId;

use crate::core::network::NetworkResult;

use super::NetworkEvent;

#[cfg(feature = "matchbox")]
pub use webrtc::{IceServer, WebRtcConfig};

/// Transport configuration for network connections.
///
/// Determines which underlying network transport to use for communication.
/// UDP is the default for lowest latency (LAN games), while WebRTC enables
/// NAT traversal for internet multiplayer.
#[derive(Debug, Clone, Default)]
pub enum TransportConfig {
  /// Direct UDP transport (default).
  ///
  /// Best for LAN games and development. Provides the lowest latency
  /// but requires direct network connectivity (no NAT traversal).
  #[default]
  Udp,

  /// WebRTC transport with NAT traversal.
  ///
  /// Enables internet multiplayer through STUN hole punching with
  /// optional TURN relay fallback. Requires a signaling server.
  ///
  /// Only available with the `matchbox` feature.
  #[cfg(feature = "matchbox")]
  WebRtc(WebRtcConfig),
}

/// Internal trait for transport implementations.
///
/// This trait abstracts the differences between UDP and WebRTC transports,
/// allowing the server/client code to work with either transparently.
#[allow(dead_code)]
pub(crate) trait ServerTransport: Send {
  /// Update the transport state.
  ///
  /// Should be called each frame to process packets and handle connections.
  fn update(&mut self, dt: Duration) -> NetworkResult<()>;

  /// Send packets from the renet server to connected clients.
  fn send_packets(&mut self, server: &mut renet::RenetServer);

  /// Get pending network events (connections, disconnections).
  fn drain_events(&mut self) -> Vec<NetworkEvent>;

  /// Check if the transport is ready to accept connections.
  fn is_ready(&self) -> bool;
}

/// Internal trait for client transport implementations.
#[allow(dead_code)]
pub(crate) trait ClientTransport: Send {
  /// Update the transport state.
  fn update(&mut self, dt: Duration) -> NetworkResult<()>;

  /// Send packets from the renet client to the server.
  fn send_packets(&mut self, client: &mut renet::RenetClient) -> NetworkResult<()>;

  /// Disconnect from the server.
  fn disconnect(&mut self);

  /// Get the disconnect reason if disconnected.
  fn disconnect_reason(&self) -> Option<String>;

  /// Get the client's unique ID.
  fn client_id(&self) -> ClientId;

  /// Check if the transport is ready (connected or connecting).
  fn is_ready(&self) -> bool;
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn transport_config_defaults_to_udp() {
    let config = TransportConfig::default();
    assert!(matches!(config, TransportConfig::Udp));
  }

  #[test]
  fn transport_config_udp_is_debug() {
    let config = TransportConfig::Udp;
    let debug = format!("{:?}", config);
    assert!(debug.contains("Udp"));
  }

  #[test]
  fn transport_config_udp_is_clone() {
    let config = TransportConfig::Udp;
    let cloned = config.clone();
    assert!(matches!(cloned, TransportConfig::Udp));
  }

  #[cfg(feature = "matchbox")]
  mod webrtc_tests {
    use super::*;

    #[test]
    fn transport_config_webrtc_variant_exists() {
      let webrtc_config = WebRtcConfig::new("ws://localhost:3536", "test-room");
      let config = TransportConfig::WebRtc(webrtc_config);
      assert!(matches!(config, TransportConfig::WebRtc(_)));
    }

    #[test]
    fn transport_config_webrtc_is_debug() {
      let webrtc_config = WebRtcConfig::new("ws://localhost:3536", "test-room");
      let config = TransportConfig::WebRtc(webrtc_config);
      let debug = format!("{:?}", config);
      assert!(debug.contains("WebRtc"));
    }

    #[test]
    fn transport_config_webrtc_is_clone() {
      let webrtc_config = WebRtcConfig::new("ws://localhost:3536", "test-room");
      let config = TransportConfig::WebRtc(webrtc_config);
      let cloned = config.clone();
      assert!(matches!(cloned, TransportConfig::WebRtc(_)));
    }
  }
}
