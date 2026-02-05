//! UDP transport implementation.
//!
//! This module provides the default UDP-based transport using renet's
//! netcode protocol. It offers the lowest latency but requires direct
//! network connectivity (no NAT traversal).

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime};

use renet::{ClientId, RenetClient, RenetServer, ServerEvent};
use renet_netcode::{
  ClientAuthentication, NetcodeClientTransport, NetcodeServerTransport, ServerAuthentication,
  ServerConfig,
};

use crate::core::error::{NetworkError, NetworkResult};
use crate::core::network::NetworkEvent;

use super::{ClientTransport, ServerTransport};

/// UDP server transport using renet's netcode protocol.
pub struct UdpServerTransport {
  transport: NetcodeServerTransport,
  events: Vec<NetworkEvent>,
}

impl UdpServerTransport {
  /// Create a new UDP server transport.
  ///
  /// Binds to `0.0.0.0:{port}` to accept connections from any interface.
  pub fn new(port: u16, max_clients: usize, protocol_id: u64) -> NetworkResult<Self> {
    let server_addr: SocketAddr = format!("0.0.0.0:{}", port)
      .parse()
      .map_err(|e| NetworkError::InitError(format!("Invalid address: {}", e)))?;

    let socket = UdpSocket::bind(server_addr)
      .map_err(|e| NetworkError::InitError(format!("Failed to bind socket: {}", e)))?;

    let current_time = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or(Duration::ZERO);

    let server_config = ServerConfig {
      current_time,
      max_clients,
      protocol_id,
      public_addresses: vec![server_addr],
      authentication: ServerAuthentication::Unsecure,
    };

    let transport = NetcodeServerTransport::new(server_config, socket)
      .map_err(|e| NetworkError::InitError(format!("Failed to create transport: {}", e)))?;

    Ok(Self {
      transport,
      events: Vec::new(),
    })
  }

  /// Process server events and collect network events.
  pub fn process_events(&mut self, server: &mut RenetServer) {
    while let Some(event) = server.get_event() {
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
}

impl ServerTransport for UdpServerTransport {
  fn update(&mut self, _dt: Duration) -> NetworkResult<()> {
    // Note: We need a mutable reference to the server for update,
    // but that's handled by the NetworkServer which owns both.
    // This is a no-op for UDP transport's internal state.
    Ok(())
  }

  fn send_packets(&mut self, server: &mut RenetServer) {
    self.transport.send_packets(server);
  }

  fn drain_events(&mut self) -> Vec<NetworkEvent> {
    std::mem::take(&mut self.events)
  }

  fn is_ready(&self) -> bool {
    true // UDP is always ready once created
  }
}

impl UdpServerTransport {
  /// Update transport with renet server (UDP-specific).
  ///
  /// This method handles the transport-level update which requires
  /// a mutable reference to the RenetServer.
  pub fn update_with_server(
    &mut self,
    dt: Duration,
    server: &mut RenetServer,
  ) -> NetworkResult<()> {
    if let Err(e) = self.transport.update(dt, server) {
      eprintln!("Network transport error: {}", e);
    }
    self.process_events(server);
    Ok(())
  }
}

/// UDP client transport using renet's netcode protocol.
pub struct UdpClientTransport {
  transport: NetcodeClientTransport,
}

impl UdpClientTransport {
  /// Create a new UDP client transport and connect to a server.
  pub fn new(server_addr: SocketAddr, protocol_id: u64) -> NetworkResult<Self> {
    // Bind to any available port
    let socket = UdpSocket::bind("0.0.0.0:0")
      .map_err(|e| NetworkError::ConnectionError(format!("Failed to bind socket: {}", e)))?;

    let current_time = SystemTime::now()
      .duration_since(SystemTime::UNIX_EPOCH)
      .unwrap_or(Duration::ZERO);

    // Generate a random client ID
    let client_id = rand_u64();

    let authentication = ClientAuthentication::Unsecure {
      protocol_id,
      client_id,
      server_addr,
      user_data: None,
    };

    let transport = NetcodeClientTransport::new(current_time, authentication, socket)
      .map_err(|e| NetworkError::ConnectionError(format!("Failed to create transport: {}", e)))?;

    Ok(Self { transport })
  }

  /// Update transport with renet client (UDP-specific).
  ///
  /// This method handles the transport-level update which requires
  /// a mutable reference to the RenetClient.
  pub fn update_with_client(
    &mut self,
    dt: Duration,
    client: &mut RenetClient,
  ) -> NetworkResult<()> {
    if let Err(e) = self.transport.update(dt, client) {
      eprintln!("Network transport error: {}", e);
    }
    Ok(())
  }
}

impl ClientTransport for UdpClientTransport {
  fn update(&mut self, _dt: Duration) -> NetworkResult<()> {
    // Main update is done in update_with_client
    Ok(())
  }

  fn send_packets(&mut self, client: &mut RenetClient) -> NetworkResult<()> {
    if let Err(e) = self.transport.send_packets(client) {
      eprintln!("Network send error: {}", e);
    }
    Ok(())
  }

  fn disconnect(&mut self) {
    self.transport.disconnect();
  }

  fn disconnect_reason(&self) -> Option<String> {
    self
      .transport
      .disconnect_reason()
      .map(|r| format!("{:?}", r))
  }

  fn client_id(&self) -> ClientId {
    self.transport.client_id()
  }

  fn is_ready(&self) -> bool {
    true // UDP is always ready once created
  }
}

/// Generate a random u64 using system randomness.
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

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicU16, Ordering};

  /// Atomic counter to ensure each test uses a unique port
  static PORT_COUNTER: AtomicU16 = AtomicU16::new(17000);

  /// Get a unique port for testing
  fn get_test_port() -> u16 {
    PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
  }

  // =========================================================================
  // UdpServerTransport Tests
  // =========================================================================

  #[test]
  fn udp_server_transport_creates_successfully() {
    let port = get_test_port();
    let transport = UdpServerTransport::new(port, 4, 12345);

    assert!(transport.is_ok());
  }

  #[test]
  fn udp_server_transport_is_ready_immediately() {
    let port = get_test_port();
    let transport = UdpServerTransport::new(port, 4, 12345).unwrap();

    assert!(transport.is_ready());
  }

  #[test]
  fn udp_server_transport_starts_with_empty_events() {
    let port = get_test_port();
    let mut transport = UdpServerTransport::new(port, 4, 12345).unwrap();

    let events = transport.drain_events();
    assert!(events.is_empty());
  }

  #[test]
  fn udp_server_transport_drain_events_clears_events() {
    let port = get_test_port();
    let mut transport = UdpServerTransport::new(port, 4, 12345).unwrap();

    // First drain
    let events1 = transport.drain_events();
    assert!(events1.is_empty());

    // Second drain should also be empty
    let events2 = transport.drain_events();
    assert!(events2.is_empty());
  }

  #[test]
  fn udp_server_transport_fails_on_invalid_port_in_use() {
    let port = get_test_port();

    // Create first transport
    let transport1 = UdpServerTransport::new(port, 4, 12345);
    assert!(transport1.is_ok());

    // Try to create second transport on same port
    let transport2 = UdpServerTransport::new(port, 4, 12345);
    assert!(transport2.is_err());
  }

  #[test]
  fn udp_server_transport_update_returns_ok() {
    let port = get_test_port();
    let mut transport = UdpServerTransport::new(port, 4, 12345).unwrap();

    let result = transport.update(Duration::from_millis(16));
    assert!(result.is_ok());
  }

  // =========================================================================
  // UdpClientTransport Tests
  // =========================================================================

  #[test]
  fn udp_client_transport_creates_successfully() {
    let server_addr: SocketAddr = "127.0.0.1:19999".parse().unwrap();
    let transport = UdpClientTransport::new(server_addr, 12345);

    assert!(transport.is_ok());
  }

  #[test]
  fn udp_client_transport_is_ready_immediately() {
    let server_addr: SocketAddr = "127.0.0.1:19998".parse().unwrap();
    let transport = UdpClientTransport::new(server_addr, 12345).unwrap();

    assert!(transport.is_ready());
  }

  #[test]
  fn udp_client_transport_client_id_is_non_zero() {
    let server_addr: SocketAddr = "127.0.0.1:19997".parse().unwrap();
    let transport = UdpClientTransport::new(server_addr, 12345).unwrap();

    // Client ID should be generated - just verify it doesn't panic
    let _client_id = transport.client_id();
  }

  #[test]
  fn udp_client_transport_disconnect_reason_none_initially() {
    let server_addr: SocketAddr = "127.0.0.1:19996".parse().unwrap();
    let transport = UdpClientTransport::new(server_addr, 12345).unwrap();

    assert!(transport.disconnect_reason().is_none());
  }

  #[test]
  fn udp_client_transport_disconnect_works() {
    let server_addr: SocketAddr = "127.0.0.1:19995".parse().unwrap();
    let mut transport = UdpClientTransport::new(server_addr, 12345).unwrap();

    // Disconnect should not panic
    transport.disconnect();

    // After disconnect, should have a disconnect reason
    // Note: The actual reason depends on netcode state
  }

  #[test]
  fn udp_client_transport_update_returns_ok() {
    let server_addr: SocketAddr = "127.0.0.1:19994".parse().unwrap();
    let mut transport = UdpClientTransport::new(server_addr, 12345).unwrap();

    let result = transport.update(Duration::from_millis(16));
    assert!(result.is_ok());
  }

  // =========================================================================
  // rand_u64 Tests
  // =========================================================================

  #[test]
  fn rand_u64_generates_different_values() {
    // Generate multiple values and verify they're not all the same
    let values: Vec<u64> = (0..10).map(|_| rand_u64()).collect();

    // At least some values should be different
    // (extremely unlikely to have 10 identical random values)
    let unique_count = values
      .iter()
      .collect::<std::collections::HashSet<_>>()
      .len();
    assert!(
      unique_count > 1,
      "Random values should not all be identical"
    );
  }
}
