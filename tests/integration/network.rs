//! Integration tests for the networking module
//!
//! These tests verify actual network communication between server and client,
//! including the `ListenServer` for player-hosted games.

use ryo2d::core::network::ReplicationError;
use ryo2d::prelude::*;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

/// Atomic counter to ensure each test uses a unique port
static PORT_COUNTER: AtomicU16 = AtomicU16::new(15000);

/// Get a unique port for testing to avoid conflicts when tests run in parallel
fn get_test_port() -> u16 {
  PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Helper to run update loops with a timeout
fn run_updates_until<F>(
  server: &mut NetworkServer,
  client: &mut NetworkClient,
  timeout_ms: u64,
  mut condition: F,
) -> bool
where
  F: FnMut(&mut NetworkServer, &mut NetworkClient) -> bool,
{
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(timeout_ms);

  while start.elapsed() < timeout {
    server.update(dt);
    client.update(dt);

    if condition(server, client) {
      return true;
    }

    thread::sleep(Duration::from_millis(10));
  }

  false
}

#[test]
fn server_starts_and_accepts_connections() {
  let port = get_test_port();

  // Start server
  let config = NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 12345,
    ..Default::default()
  };
  let mut server = NetworkServer::new(config).expect("Failed to create server");
  assert!(server.is_running());
  assert_eq!(server.connected_count(), 0);

  // Connect client
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let client_config = NetworkClientConfig::udp(server_addr, 12345);
  let mut client = NetworkClient::connect(client_config).expect("Failed to create client");

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });

  assert!(connected, "Client failed to connect within timeout");
  assert!(client.is_connected());
  assert_eq!(server.connected_count(), 1);
}

#[test]
fn client_receives_connected_event() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 54321,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 54321)).unwrap();

  let mut got_connected_event = false;

  let success = run_updates_until(&mut server, &mut client, 2000, |_server, client| {
    for event in client.events() {
      if matches!(event, NetworkEvent::Connected) {
        got_connected_event = true;
        return true;
      }
    }
    false
  });

  assert!(success, "Did not receive Connected event within timeout");
  assert!(got_connected_event);
}

#[test]
fn server_receives_client_connected_event() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 11111,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 11111)).unwrap();

  let mut got_client_connected = false;
  let mut connected_client_id = None;

  let success = run_updates_until(&mut server, &mut client, 2000, |server, _client| {
    for event in server.events() {
      if let NetworkEvent::ClientConnected { client_id } = event {
        got_client_connected = true;
        connected_client_id = Some(client_id);
        return true;
      }
    }
    false
  });

  assert!(
    success,
    "Did not receive ClientConnected event within timeout"
  );
  assert!(got_client_connected);
  assert!(connected_client_id.is_some());
}

#[test]
fn client_sends_message_to_server() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 22222,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 22222)).unwrap();

  // Wait for connection first
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected, "Failed to establish connection");

  // Get the client ID
  let client_ids: Vec<_> = server.clients().collect();
  assert_eq!(client_ids.len(), 1);
  let client_id = client_ids[0];

  // Send message from client
  let message = b"Hello from client!";
  client.send(Channel::ReliableOrdered, message);

  // Wait for server to receive it
  let mut received_message = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
      received_message = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(received_message.is_some(), "Server did not receive message");
  assert_eq!(received_message.unwrap(), message.to_vec());
}

#[test]
fn server_sends_message_to_client() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 33333,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 33333)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected, "Failed to establish connection");

  // Get client ID and send message
  let client_ids: Vec<_> = server.clients().collect();
  let client_id = client_ids[0];

  let message = b"Hello from server!";
  server.send(client_id, Channel::ReliableOrdered, message);

  // Wait for client to receive it
  let mut received_message = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = client.receive(Channel::ReliableOrdered) {
      received_message = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(received_message.is_some(), "Client did not receive message");
  assert_eq!(received_message.unwrap(), message.to_vec());
}

#[test]
fn server_broadcasts_to_multiple_clients() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 44444,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

  // Connect two clients
  let mut client1 = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 44444)).unwrap();

  let mut client2 = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 44444)).unwrap();

  // Wait for both to connect
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(3000);

  while start.elapsed() < timeout {
    server.update(dt);
    client1.update(dt);
    client2.update(dt);

    if client1.is_connected() && client2.is_connected() && server.connected_count() == 2 {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert_eq!(
    server.connected_count(),
    2,
    "Both clients should be connected"
  );

  // Broadcast message
  let message = b"Broadcast message!";
  server.broadcast(Channel::ReliableOrdered, message);

  // Wait for both clients to receive
  let mut client1_received = None;
  let mut client2_received = None;

  let start = std::time::Instant::now();
  while start.elapsed() < timeout {
    server.update(dt);
    client1.update(dt);
    client2.update(dt);

    if client1_received.is_none()
      && let Some(data) = client1.receive(Channel::ReliableOrdered)
    {
      client1_received = Some(data);
    }

    if client2_received.is_none()
      && let Some(data) = client2.receive(Channel::ReliableOrdered)
    {
      client2_received = Some(data);
    }

    if client1_received.is_some() && client2_received.is_some() {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    client1_received.is_some(),
    "Client 1 did not receive broadcast"
  );
  assert!(
    client2_received.is_some(),
    "Client 2 did not receive broadcast"
  );
  assert_eq!(client1_received.unwrap(), message.to_vec());
  assert_eq!(client2_received.unwrap(), message.to_vec());
}

#[test]
fn unreliable_channel_works() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 55555,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 55555)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected, "Failed to establish connection");

  // Send multiple unreliable messages (some might be lost, but at least one should arrive)
  for i in 0..10 {
    let message = format!("Unreliable message {}", i);
    client.send(Channel::Unreliable, message.as_bytes());
  }

  let client_ids: Vec<_> = server.clients().collect();
  let client_id = client_ids[0];

  // Wait and collect any received messages
  let mut received_count = 0;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    while server.receive(client_id, Channel::Unreliable).is_some() {
      received_count += 1;
    }

    thread::sleep(Duration::from_millis(10));
  }

  // At least some messages should arrive (unreliable doesn't guarantee all)
  assert!(
    received_count > 0,
    "No unreliable messages received (received {})",
    received_count
  );
}

#[test]
fn protocol_id_mismatch_prevents_connection() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 99999, // Server uses this ID
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 11111)) // Client uses different ID
    .unwrap();

  // Try to connect - should fail due to protocol mismatch
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(2000);

  while start.elapsed() < timeout {
    server.update(dt);
    client.update(dt);
    thread::sleep(Duration::from_millis(10));
  }

  // Connection should NOT be established
  assert!(
    !client.is_connected(),
    "Client should not connect with mismatched protocol ID"
  );
  assert_eq!(
    server.connected_count(),
    0,
    "Server should have no connected clients"
  );
}

#[test]
fn client_disconnect_generates_event() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 66666,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 66666)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected, "Failed to establish connection");

  // Disconnect client
  client.disconnect();

  // Wait for server to receive disconnect event
  let mut got_disconnect_event = false;
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(2000);

  while start.elapsed() < timeout {
    server.update(dt);
    client.update(dt);

    for event in server.events() {
      if matches!(event, NetworkEvent::ClientDisconnected { .. }) {
        got_disconnect_event = true;
        break;
      }
    }

    if got_disconnect_event {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    got_disconnect_event,
    "Server did not receive ClientDisconnected event"
  );
}

// =============================================================================
// ListenServer Tests
// =============================================================================

/// Helper to run update loops for ListenServer with a timeout
fn run_listen_server_updates_until<F>(
  server: &mut ListenServer,
  client: &mut NetworkClient,
  timeout_ms: u64,
  mut condition: F,
) -> bool
where
  F: FnMut(&mut ListenServer, &mut NetworkClient) -> bool,
{
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(timeout_ms);

  while start.elapsed() < timeout {
    server.update(dt);
    client.update(dt);

    if condition(server, client) {
      return true;
    }

    thread::sleep(Duration::from_millis(10));
  }

  false
}

#[test]
fn listen_server_creates_successfully() {
  let port = get_test_port();

  let config = NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77777,
    ..Default::default()
  };

  let server = ListenServer::new(config).expect("Failed to create listen server");

  assert!(server.is_running());
  assert_eq!(server.host_id(), LOCAL_HOST_ID);
  assert_eq!(server.player_count(), 1); // Host is always counted
  assert_eq!(server.remote_count(), 0);
}

#[test]
fn listen_server_host_id_is_local_host_id() {
  let port = get_test_port();

  let server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77778,
    ..Default::default()
  })
  .unwrap();

  assert_eq!(server.host_id(), LOCAL_HOST_ID);
  assert!(server.is_host(LOCAL_HOST_ID));
  assert!(!server.is_host(1)); // ClientId is just u64
}

#[test]
fn listen_server_remote_client_connects() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77779,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77779)).unwrap();

  // Wait for connection
  let connected =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, client| {
      client.is_connected() && server.remote_count() == 1
    });

  assert!(connected, "Remote client failed to connect");
  assert!(client.is_connected());
  assert_eq!(server.remote_count(), 1);
  assert_eq!(server.player_count(), 2); // Host + 1 remote
}

#[test]
fn listen_server_generates_client_connected_event() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77780,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77780)).unwrap();

  let mut got_client_connected = false;

  let success =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, _client| {
      for event in server.events() {
        if matches!(event, NetworkEvent::ClientConnected { .. }) {
          got_client_connected = true;
          return true;
        }
      }
      false
    });

  assert!(
    success,
    "Did not receive ClientConnected event within timeout"
  );
  assert!(got_client_connected);
}

#[test]
fn listen_server_all_clients_includes_host_and_remotes() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77781,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77781)).unwrap();

  // Wait for connection
  let connected =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, client| {
      client.is_connected() && server.remote_count() == 1
    });

  assert!(connected, "Remote client failed to connect");

  // Check all_clients includes both host and remote
  let all_clients: Vec<_> = server.all_clients().collect();
  assert_eq!(all_clients.len(), 2);
  assert_eq!(all_clients[0], LOCAL_HOST_ID); // Host is first
  assert!(!server.is_host(all_clients[1])); // Second is remote

  // Check remote_clients excludes host
  let remote_clients: Vec<_> = server.remote_clients().collect();
  assert_eq!(remote_clients.len(), 1);
  assert!(!server.is_host(remote_clients[0]));
}

#[test]
fn listen_server_sends_to_remote_client() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77782,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77782)).unwrap();

  // Wait for connection
  let connected =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, client| {
      client.is_connected() && server.remote_count() == 1
    });
  assert!(connected, "Failed to establish connection");

  // Get the remote client ID
  let remote_clients: Vec<_> = server.remote_clients().collect();
  let remote_id = remote_clients[0];

  // Send message to remote client
  let message = b"Hello from listen server!";
  server.send(remote_id, Channel::ReliableOrdered, message);

  // Wait for client to receive it
  let mut received_message = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = client.receive(Channel::ReliableOrdered) {
      received_message = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    received_message.is_some(),
    "Remote client did not receive message"
  );
  assert_eq!(received_message.unwrap(), message.to_vec());
}

#[test]
fn listen_server_receives_from_remote_client() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77783,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77783)).unwrap();

  // Wait for connection
  let connected =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, client| {
      client.is_connected() && server.remote_count() == 1
    });
  assert!(connected, "Failed to establish connection");

  // Get remote client ID
  let remote_clients: Vec<_> = server.remote_clients().collect();
  let remote_id = remote_clients[0];

  // Send message from client
  let message = b"Hello from remote client!";
  client.send(Channel::ReliableOrdered, message);

  // Wait for server to receive it
  let mut received_message = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = server.receive(remote_id, Channel::ReliableOrdered) {
      received_message = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    received_message.is_some(),
    "Listen server did not receive message from remote"
  );
  assert_eq!(received_message.unwrap(), message.to_vec());
}

#[test]
fn listen_server_broadcast_to_remotes() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77784,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

  // Connect two remote clients
  let mut client1 = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77784)).unwrap();

  let mut client2 = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 77784)).unwrap();

  // Wait for both to connect
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(3000);

  while start.elapsed() < timeout {
    server.update(dt);
    client1.update(dt);
    client2.update(dt);

    if client1.is_connected() && client2.is_connected() && server.remote_count() == 2 {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert_eq!(server.remote_count(), 2, "Both clients should be connected");
  assert_eq!(server.player_count(), 3); // Host + 2 remotes

  // Broadcast message
  let message = b"Broadcast from listen server!";
  server.broadcast(Channel::ReliableOrdered, message);

  // Wait for both clients to receive
  let mut client1_received = None;
  let mut client2_received = None;

  let start = std::time::Instant::now();
  while start.elapsed() < timeout {
    server.update(dt);
    client1.update(dt);
    client2.update(dt);

    if client1_received.is_none()
      && let Some(data) = client1.receive(Channel::ReliableOrdered)
    {
      client1_received = Some(data);
    }

    if client2_received.is_none()
      && let Some(data) = client2.receive(Channel::ReliableOrdered)
    {
      client2_received = Some(data);
    }

    if client1_received.is_some() && client2_received.is_some() {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    client1_received.is_some(),
    "Client 1 did not receive broadcast"
  );
  assert!(
    client2_received.is_some(),
    "Client 2 did not receive broadcast"
  );
  assert_eq!(client1_received.unwrap(), message.to_vec());
  assert_eq!(client2_received.unwrap(), message.to_vec());
}

#[test]
fn listen_server_receive_from_host_returns_none() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 77785,
    ..Default::default()
  })
  .unwrap();

  // Attempting to receive from the host should return None
  // (host messages are not networked)
  assert!(
    server
      .receive(LOCAL_HOST_ID, Channel::ReliableOrdered)
      .is_none()
  );
  assert!(server.receive(LOCAL_HOST_ID, Channel::Unreliable).is_none());
}

// =============================================================================
// Replication Tests
// =============================================================================

use serde::{Deserialize, Serialize};

/// Test component for replication tests
#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestHealth {
  current: f32,
  max: f32,
}

impl Replicated for TestHealth {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("TestHealth")
  }

  fn component_name() -> &'static str {
    "TestHealth"
  }

  fn replication_priority() -> u8 {
    5
  }
}

/// Test component with higher priority
#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestScore {
  points: i32,
}

impl Replicated for TestScore {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("TestScore")
  }

  fn component_name() -> &'static str {
    "TestScore"
  }

  fn replication_priority() -> u8 {
    1
  }
}

#[test]
fn network_id_generates_unique_ids() {
  let id1 = NetworkId::new();
  let id2 = NetworkId::new();
  let id3 = NetworkId::new();

  assert_ne!(id1, id2);
  assert_ne!(id2, id3);
  assert_ne!(id1, id3);

  // IDs should be sequential
  assert!(id2.0 > id1.0);
  assert!(id3.0 > id2.0);
}

#[test]
fn component_type_id_consistent_for_same_name() {
  let id1 = ComponentTypeId::from_name("Transform");
  let id2 = ComponentTypeId::from_name("Transform");
  let id3 = ComponentTypeId::from_name("Velocity");

  assert_eq!(id1, id2);
  assert_ne!(id1, id3);
}

#[test]
fn component_registry_registers_and_serializes() {
  let mut registry = ComponentRegistry::new();
  registry.register::<Transform>();
  registry.register::<Velocity>();

  assert!(registry.is_registered::<Transform>());
  assert!(registry.is_registered::<Velocity>());
  assert!(!registry.is_registered::<TestHealth>());

  // Test serialization using SerializedComponent directly
  let transform = Transform::from_xy(100.0, 200.0);
  let serialized = SerializedComponent::from_component(&transform).unwrap();

  assert_eq!(serialized.type_id, Transform::component_id());

  // Test deserialization
  let deserialized: Transform = serialized.into_component().unwrap();
  assert_eq!(deserialized.translation.x, 100.0);
  assert_eq!(deserialized.translation.y, 200.0);
}

#[test]
fn component_registry_handles_custom_components() {
  let mut registry = ComponentRegistry::new();
  registry.register::<TestHealth>();
  registry.register::<TestScore>();

  let health = TestHealth {
    current: 75.0,
    max: 100.0,
  };
  let serialized = SerializedComponent::from_component(&health).unwrap();
  let deserialized: TestHealth = serialized.into_component().unwrap();

  assert_eq!(deserialized.current, 75.0);
  assert_eq!(deserialized.max, 100.0);
}

#[test]
fn dirty_tracker_tracks_component_changes() {
  let mut tracker = DirtyTracker::new();

  let net_id1 = NetworkId::from_raw(1);
  let net_id2 = NetworkId::from_raw(2);
  let transform_id = ComponentTypeId::from_name("Transform");
  let velocity_id = ComponentTypeId::from_name("Velocity");

  // Mark components as dirty
  tracker.mark_dirty(net_id1, transform_id);
  tracker.mark_dirty(net_id1, velocity_id);
  tracker.mark_dirty(net_id2, transform_id);

  // Check dirty status for new client (unregistered = sees everything)
  let dirty = tracker.get_dirty_for_client(1);
  assert!(!dirty.is_empty(), "Should have dirty components");

  // Find net_id1's dirty components
  let net_id1_dirty = dirty.iter().find(|(id, _)| *id == net_id1);
  assert!(net_id1_dirty.is_some());
  let (_, components) = net_id1_dirty.unwrap();
  assert!(components.contains(&transform_id));
  assert!(components.contains(&velocity_id));

  // Register and acknowledge client
  tracker.register_client(1);
  tracker.acknowledge_client(1);

  // Move to next generation
  tracker.next_generation();

  // After acknowledgment and generation advance, client shouldn't see old changes
  let dirty_after_ack = tracker.get_dirty_for_client(1);
  assert!(
    dirty_after_ack.is_empty(),
    "Should not have dirty components after ack"
  );

  // Mark something new
  tracker.mark_dirty(net_id1, transform_id);

  // Now client should see it
  let dirty_new = tracker.get_dirty_for_client(1);
  assert!(!dirty_new.is_empty(), "New change should be dirty");
}

#[test]
fn dirty_tracker_handles_spawns_and_despawns() {
  let mut tracker = DirtyTracker::new();
  let net_id = NetworkId::from_raw(1);

  tracker.mark_spawned(net_id);
  assert!(tracker.is_spawned(net_id));

  tracker.mark_despawned(net_id);
  assert!(tracker.is_despawned(net_id));
}

#[test]
fn network_entity_builder_creates_server_owned_entity() {
  let mut world = World::new();
  let mut manager = ReplicationManager::new_server();
  manager.register::<Transform>();

  // NetworkEntityBuilder doesn't add components directly - it just sets network metadata
  // Components are added separately after spawning
  let entity = manager.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());

  // Add component after spawning
  world
    .entity_mut(entity)
    .insert(Transform::from_xy(50.0, 75.0));

  // Verify entity was created
  assert!(world.get::<Transform>(entity).is_some());
  assert!(world.get::<NetworkEntity>(entity).is_some());

  let network_entity = world.get::<NetworkEntity>(entity).unwrap();
  assert_eq!(network_entity.authority, Authority::Server);
  assert_eq!(network_entity.owner, 0);
}

#[test]
fn network_entity_builder_creates_client_owned_entity() {
  let mut world = World::new();
  let mut manager = ReplicationManager::new_server();
  manager.register::<Transform>();

  let client_id: ClientId = 42;
  let entity = manager.spawn_networked(
    &mut world,
    NetworkEntityBuilder::new().client_owned(client_id),
  );

  // Add component after spawning
  world
    .entity_mut(entity)
    .insert(Transform::from_xy(100.0, 200.0));

  let network_entity = world.get::<NetworkEntity>(entity).unwrap();
  assert_eq!(network_entity.authority, Authority::Client(client_id));
  assert_eq!(network_entity.owner, client_id);
}

#[test]
fn replication_manager_tracks_network_ids() {
  let mut world = World::new();
  let mut manager = ReplicationManager::new_server();
  manager.register::<Transform>();

  let entity = manager.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());
  world
    .entity_mut(entity)
    .insert(Transform::from_xy(0.0, 0.0));

  // Get network ID from entity
  let network_id = manager.get_network_id(entity);
  assert!(network_id.is_some());

  // Get entity from network ID
  let found_entity = manager.get_entity(network_id.unwrap());
  assert_eq!(found_entity, Some(entity));
}

#[test]
fn replication_manager_despawns_entities() {
  let mut world = World::new();
  let mut manager = ReplicationManager::new_server();
  manager.register::<Transform>();

  let entity = manager.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());
  world
    .entity_mut(entity)
    .insert(Transform::from_xy(0.0, 0.0));

  let network_id = manager.get_network_id(entity).unwrap();

  // Despawn the entity
  manager.despawn_networked(&mut world, entity);

  // Entity should be gone from world
  assert!(world.get::<Transform>(entity).is_none());

  // Network ID mapping should be removed
  assert!(manager.get_entity(network_id).is_none());
}

#[test]
fn replication_manager_marks_components_dirty() {
  let mut world = World::new();
  let mut manager = ReplicationManager::new_server();
  manager.register::<Transform>();
  manager.register::<Velocity>();

  let entity = manager.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());
  world
    .entity_mut(entity)
    .insert(Transform::from_xy(0.0, 0.0));
  world.entity_mut(entity).insert(Velocity::new(1.0, 2.0));

  // Modify and mark dirty
  if let Some(mut transform) = world.get_mut::<Transform>(entity) {
    transform.translation.x = 100.0;
  }
  manager.mark_dirty::<Transform>(entity);

  // Check dirty tracker has something dirty
  let network_id = manager.get_network_id(entity).unwrap();
  let dirty = manager.dirty_tracker().get_dirty_for_client(0);
  let entity_dirty = dirty.iter().find(|(id, _)| *id == network_id);
  assert!(
    entity_dirty.is_some(),
    "Entity should have dirty components"
  );
}

#[test]
fn spawn_entity_message_serializes_correctly() {
  let network_id = NetworkId::new();
  let transform = Transform::from_xy(100.0, 200.0);

  let serialized_transform = SerializedComponent::from_component(&transform).unwrap();

  let msg = SpawnEntityMessage::new(network_id, 1, Authority::Client(1))
    .with_component(serialized_transform);

  // Serialize to bytes
  let bytes = serde_json::to_vec(&msg).unwrap();
  assert!(!bytes.is_empty());

  // Deserialize back
  let deserialized: SpawnEntityMessage = serde_json::from_slice(&bytes).unwrap();
  assert_eq!(deserialized.network_id, network_id);
  assert_eq!(deserialized.owner, 1);
  assert_eq!(deserialized.authority, Authority::Client(1));
  assert_eq!(deserialized.components.len(), 1);
}

#[test]
fn component_batch_message_serializes_correctly() {
  let network_id = NetworkId::new();

  let transform = SerializedComponent::from_component(&Transform::from_xy(50.0, 75.0)).unwrap();
  let velocity = SerializedComponent::from_component(&Velocity::new(1.0, 2.0)).unwrap();

  let msg = ComponentBatchMessage {
    updates: vec![
      ComponentUpdateMessage::new(network_id, transform),
      ComponentUpdateMessage::new(network_id, velocity),
    ],
    generation: 0,
  };

  let bytes = serde_json::to_vec(&msg).unwrap();
  let deserialized: ComponentBatchMessage = serde_json::from_slice(&bytes).unwrap();

  assert_eq!(deserialized.updates.len(), 2);
}

#[test]
fn full_state_sync_message_serializes_correctly() {
  let mut sync = FullStateSyncMessage::new(0);

  for i in 0..3 {
    let network_id = NetworkId::new();
    let transform =
      SerializedComponent::from_component(&Transform::from_xy(i as f32 * 100.0, 0.0)).unwrap();

    let spawn_msg =
      SpawnEntityMessage::new(network_id, 0, Authority::Server).with_component(transform);
    sync.add_entity(spawn_msg);
  }

  let bytes = serde_json::to_vec(&sync).unwrap();
  let deserialized: FullStateSyncMessage = serde_json::from_slice(&bytes).unwrap();

  assert_eq!(deserialized.entities.len(), 3);
}

#[test]
fn replication_message_enum_wraps_all_types() {
  let network_id = NetworkId::new();

  // Test SpawnEntity variant
  let spawn = ReplicationMessage::SpawnEntity(SpawnEntityMessage {
    network_id,
    owner: 0,
    authority: Authority::Server,
    components: vec![],
  });
  let bytes = serde_json::to_vec(&spawn).unwrap();
  let _: ReplicationMessage = serde_json::from_slice(&bytes).unwrap();

  // Test DespawnEntity variant
  let despawn = ReplicationMessage::DespawnEntity(DespawnEntityMessage { network_id });
  let bytes = serde_json::to_vec(&despawn).unwrap();
  let _: ReplicationMessage = serde_json::from_slice(&bytes).unwrap();

  // Test ComponentBatch variant
  let batch = ReplicationMessage::ComponentBatch(ComponentBatchMessage {
    updates: vec![],
    generation: 0,
  });
  let bytes = serde_json::to_vec(&batch).unwrap();
  let _: ReplicationMessage = serde_json::from_slice(&bytes).unwrap();
}

// =============================================================================
// RPC Tests
// =============================================================================

/// Test RPC type for integration tests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestPingRpc {
  sequence: u32,
}

impl Rpc for TestPingRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("TestPingRpc")
  }

  fn rpc_name() -> &'static str {
    "TestPingRpc"
  }
}

/// Test RPC with payload data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestPlayerInputRpc {
  movement_x: f32,
  movement_y: f32,
  jump: bool,
  attack: bool,
}

impl Rpc for TestPlayerInputRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("TestPlayerInputRpc")
  }

  fn rpc_name() -> &'static str {
    "TestPlayerInputRpc"
  }
}

/// Test RPC for chat messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestChatRpc {
  message: String,
}

impl Rpc for TestChatRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("TestChatRpc")
  }

  fn rpc_name() -> &'static str {
    "TestChatRpc"
  }
}

#[test]
fn rpc_type_id_consistent_for_same_name() {
  let id1 = RpcTypeId::from_name("TestRpc");
  let id2 = RpcTypeId::from_name("TestRpc");
  let id3 = RpcTypeId::from_name("OtherRpc");

  assert_eq!(id1, id2);
  assert_ne!(id1, id3);
}

#[test]
fn serialized_rpc_roundtrip() {
  let rpc = TestPingRpc { sequence: 42 };
  let sender: ClientId = 1;
  let target = RpcTarget::Server;

  // Serialize
  let serialized = SerializedRpc::from_rpc(&rpc, sender, target).unwrap();

  assert_eq!(serialized.rpc_id, TestPingRpc::rpc_id());
  assert_eq!(serialized.sender, sender);
  assert!(matches!(serialized.target, RpcTarget::Server));

  // Convert to bytes and back
  let bytes = serialized.to_bytes().unwrap();
  let restored = SerializedRpc::from_bytes(&bytes).unwrap();

  assert_eq!(restored.rpc_id, TestPingRpc::rpc_id());
  assert_eq!(restored.sender, sender);

  // Deserialize payload
  let received: ReceivedRpc<TestPingRpc> = restored.into_rpc().unwrap();
  assert_eq!(received.payload.sequence, 42);
  assert_eq!(received.sender, sender);
}

#[test]
fn serialized_rpc_handles_complex_payload() {
  let rpc = TestPlayerInputRpc {
    movement_x: 1.0,
    movement_y: -0.5,
    jump: true,
    attack: false,
  };

  let serialized = SerializedRpc::from_rpc(&rpc, 5, RpcTarget::Server).unwrap();
  let bytes = serialized.to_bytes().unwrap();
  let restored = SerializedRpc::from_bytes(&bytes).unwrap();
  let received: ReceivedRpc<TestPlayerInputRpc> = restored.into_rpc().unwrap();

  assert_eq!(received.payload.movement_x, 1.0);
  assert_eq!(received.payload.movement_y, -0.5);
  assert!(received.payload.jump);
  assert!(!received.payload.attack);
}

#[test]
fn serialized_rpc_handles_string_payload() {
  let rpc = TestChatRpc {
    message: "Hello, world!".to_string(),
  };

  let serialized = SerializedRpc::from_rpc(&rpc, 10, RpcTarget::Broadcast).unwrap();
  let bytes = serialized.to_bytes().unwrap();
  let restored = SerializedRpc::from_bytes(&bytes).unwrap();
  let received: ReceivedRpc<TestChatRpc> = restored.into_rpc().unwrap();

  assert_eq!(received.payload.message, "Hello, world!");
}

#[test]
fn rpc_target_variants() {
  // Test all RpcTarget variants serialize correctly
  let targets = [
    RpcTarget::Server,
    RpcTarget::Client(42),
    RpcTarget::Broadcast,
    RpcTarget::BroadcastExcept(7),
  ];

  for target in targets {
    let rpc = TestPingRpc { sequence: 1 };
    let serialized = SerializedRpc::from_rpc(&rpc, 0, target).unwrap();
    let bytes = serialized.to_bytes().unwrap();
    let restored = SerializedRpc::from_bytes(&bytes).unwrap();

    match (&target, &restored.target) {
      (RpcTarget::Server, RpcTarget::Server) => {}
      (RpcTarget::Client(a), RpcTarget::Client(b)) => assert_eq!(a, b),
      (RpcTarget::Broadcast, RpcTarget::Broadcast) => {}
      (RpcTarget::BroadcastExcept(a), RpcTarget::BroadcastExcept(b)) => assert_eq!(a, b),
      _ => panic!("Target mismatch"),
    }
  }
}

#[test]
fn rpc_registry_registers_handlers() {
  let mut registry = RpcRegistry::new();

  registry.register::<TestPingRpc, _>(|_world, rpc| {
    assert_eq!(rpc.payload.sequence, 100);
    Ok(())
  });

  assert!(registry.is_registered::<TestPingRpc>());
  assert!(!registry.is_registered::<TestChatRpc>());
}

#[test]
fn rpc_registry_dispatches_to_handler() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicU32, Ordering};

  let mut registry = RpcRegistry::new();
  let call_count = Arc::new(AtomicU32::new(0));
  let call_count_clone = call_count.clone();

  registry.register::<TestPingRpc, _>(move |_world, rpc| {
    assert_eq!(rpc.payload.sequence, 999);
    call_count_clone.fetch_add(1, Ordering::SeqCst);
    Ok(())
  });

  let rpc = TestPingRpc { sequence: 999 };
  let serialized = SerializedRpc::from_rpc(&rpc, 1, RpcTarget::Server).unwrap();

  let mut world = World::new();
  registry.dispatch(&mut world, &serialized).unwrap();

  assert_eq!(call_count.load(Ordering::SeqCst), 1);
}

#[test]
fn rpc_registry_returns_error_for_unknown_rpc() {
  let registry = RpcRegistry::new();

  let rpc = TestPingRpc { sequence: 1 };
  let serialized = SerializedRpc::from_rpc(&rpc, 1, RpcTarget::Server).unwrap();

  let mut world = World::new();
  let result = registry.dispatch(&mut world, &serialized);

  assert!(result.is_err());
  assert!(matches!(result, Err(ReplicationError::RpcNotFound(_))));
}

#[test]
fn rpc_registry_dispatches_multiple_types() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicU32, Ordering};

  let mut registry = RpcRegistry::new();

  let ping_count = Arc::new(AtomicU32::new(0));
  let ping_count_clone = ping_count.clone();

  let chat_count = Arc::new(AtomicU32::new(0));
  let chat_count_clone = chat_count.clone();

  registry.register::<TestPingRpc, _>(move |_world, _rpc| {
    ping_count_clone.fetch_add(1, Ordering::SeqCst);
    Ok(())
  });

  registry.register::<TestChatRpc, _>(move |_world, _rpc| {
    chat_count_clone.fetch_add(1, Ordering::SeqCst);
    Ok(())
  });

  let mut world = World::new();

  // Dispatch ping
  let ping = SerializedRpc::from_rpc(&TestPingRpc { sequence: 1 }, 1, RpcTarget::Server).unwrap();
  registry.dispatch(&mut world, &ping).unwrap();

  // Dispatch chat
  let chat = SerializedRpc::from_rpc(
    &TestChatRpc {
      message: "hi".to_string(),
    },
    2,
    RpcTarget::Server,
  )
  .unwrap();
  registry.dispatch(&mut world, &chat).unwrap();

  // Dispatch another ping
  let ping2 = SerializedRpc::from_rpc(&TestPingRpc { sequence: 2 }, 1, RpcTarget::Server).unwrap();
  registry.dispatch(&mut world, &ping2).unwrap();

  assert_eq!(ping_count.load(Ordering::SeqCst), 2);
  assert_eq!(chat_count.load(Ordering::SeqCst), 1);
}

// =============================================================================
// Network Integration Tests for Replication and RPC
// =============================================================================

#[test]
fn replication_server_broadcasts_spawn_to_client() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88880,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88880)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected, "Failed to establish connection");

  // Setup replication on server
  let mut server_world = World::new();
  let mut server_replication = ReplicationManager::new_server();
  server_replication.register::<Transform>();

  // Spawn a networked entity
  let entity = server_replication.spawn_networked(
    &mut server_world,
    NetworkEntityBuilder::new().server_owned(),
  );
  server_world
    .entity_mut(entity)
    .insert(Transform::from_xy(123.0, 456.0));

  let network_id = server_replication.get_network_id(entity).unwrap();

  // Get client ID
  let client_ids: Vec<_> = server.clients().collect();
  let client_id = client_ids[0];

  // Send full state to client (no Result, just call it)
  server_replication.send_full_state(&server_world, &mut server, client_id);

  // Wait for the marker and the entity's spawn
  let mut received = Vec::new();
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    while let Some(data) = client.receive(Channel::ReplicationReliable) {
      received.push(data);
    }
    if received.len() >= 2 {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert_eq!(received.len(), 2, "Client did not receive the full state");

  let marker = ReplicationMessage::from_bytes(&received[0]).unwrap();
  if let ReplicationMessage::FullStateSync(sync) = marker {
    assert!(sync.entities.is_empty());
  } else {
    panic!("Expected FullStateSync message");
  }

  let spawn = ReplicationMessage::from_bytes(&received[1]).unwrap();
  if let ReplicationMessage::SpawnEntity(spawn) = spawn {
    assert_eq!(spawn.network_id, network_id);
    assert_eq!(spawn.authority, Authority::Server);
  } else {
    panic!("Expected SpawnEntity message");
  }
}

#[test]
fn replication_client_receives_component_updates() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88881,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88881)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected);

  // Setup replication
  let mut server_world = World::new();
  let mut server_replication = ReplicationManager::new_server();
  server_replication.register::<Transform>();

  let entity = server_replication.spawn_networked(
    &mut server_world,
    NetworkEntityBuilder::new().server_owned(),
  );
  server_world
    .entity_mut(entity)
    .insert(Transform::from_xy(0.0, 0.0));

  // Modify the transform
  if let Some(mut transform) = server_world.get_mut::<Transform>(entity) {
    transform.translation.x = 500.0;
    transform.translation.y = 600.0;
  }
  server_replication.mark_dirty::<Transform>(entity);

  // Broadcast updates (no Result, just call it)
  server_replication.broadcast_updates(&server_world, &mut server);

  // Wait for client to receive - could be spawn or batch
  let mut received_data = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    // Check reliable channel for spawns
    if let Some(data) = client.receive(Channel::ReplicationReliable) {
      received_data = Some(data);
      break;
    }

    // Check unreliable channel for component updates
    if let Some(data) = client.receive(Channel::ReplicationUnreliable) {
      received_data = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    received_data.is_some(),
    "Client did not receive any replication data"
  );

  // Parse the message - could be spawn or batch
  let msg = ReplicationMessage::from_bytes(&received_data.unwrap()).unwrap();

  match msg {
    ReplicationMessage::SpawnEntity(_) => {
      // Got spawn message - that's expected for new entity
    }
    ReplicationMessage::ComponentBatch(batch) => {
      assert!(!batch.updates.is_empty());
    }
    _ => panic!("Expected SpawnEntity or ComponentBatch message"),
  }
}

#[test]
fn rpc_client_sends_to_server() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88882,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88882)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected);

  // Client sends RPC
  let rpc = TestPlayerInputRpc {
    movement_x: 1.0,
    movement_y: -1.0,
    jump: true,
    attack: false,
  };
  let serialized = SerializedRpc::from_rpc(&rpc, client.client_id(), RpcTarget::Server).unwrap();
  client.send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());

  // Get client ID on server
  let client_ids: Vec<_> = server.clients().collect();
  let client_id = client_ids[0];

  // Wait for server to receive
  let mut received_data = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
      received_data = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(received_data.is_some(), "Server did not receive RPC");

  // Parse the RPC
  let serialized = SerializedRpc::from_bytes(&received_data.unwrap()).unwrap();
  let received: ReceivedRpc<TestPlayerInputRpc> = serialized.into_rpc().unwrap();

  assert_eq!(received.payload.movement_x, 1.0);
  assert_eq!(received.payload.movement_y, -1.0);
  assert!(received.payload.jump);
  assert!(!received.payload.attack);
}

#[test]
fn rpc_server_broadcasts_to_clients() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88883,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

  let mut client1 = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88883)).unwrap();

  let mut client2 = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88883)).unwrap();

  // Wait for both to connect
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(3000);

  while start.elapsed() < timeout {
    server.update(dt);
    client1.update(dt);
    client2.update(dt);

    if client1.is_connected() && client2.is_connected() && server.connected_count() == 2 {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }
  assert_eq!(server.connected_count(), 2);

  // Server broadcasts RPC
  let rpc = TestChatRpc {
    message: "Hello everyone!".to_string(),
  };
  let serialized = SerializedRpc::from_rpc(&rpc, 0, RpcTarget::Broadcast).unwrap();
  server.broadcast(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());

  // Wait for both clients to receive
  let mut client1_received = None;
  let mut client2_received = None;

  let start = std::time::Instant::now();
  while start.elapsed() < timeout {
    server.update(dt);
    client1.update(dt);
    client2.update(dt);

    if client1_received.is_none()
      && let Some(data) = client1.receive(Channel::ReliableOrdered)
    {
      client1_received = Some(data);
    }

    if client2_received.is_none()
      && let Some(data) = client2.receive(Channel::ReliableOrdered)
    {
      client2_received = Some(data);
    }

    if client1_received.is_some() && client2_received.is_some() {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(client1_received.is_some(), "Client 1 did not receive RPC");
  assert!(client2_received.is_some(), "Client 2 did not receive RPC");

  // Verify both received the same message
  let rpc1: ReceivedRpc<TestChatRpc> = SerializedRpc::from_bytes(&client1_received.unwrap())
    .unwrap()
    .into_rpc()
    .unwrap();
  let rpc2: ReceivedRpc<TestChatRpc> = SerializedRpc::from_bytes(&client2_received.unwrap())
    .unwrap()
    .into_rpc()
    .unwrap();

  assert_eq!(rpc1.payload.message, "Hello everyone!");
  assert_eq!(rpc2.payload.message, "Hello everyone!");
}

#[test]
fn rpc_registry_processes_network_rpcs() {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicU32, Ordering};

  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88884,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88884)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected);

  // Setup RPC registry on server
  let mut registry = RpcRegistry::new();
  let received_sequence = Arc::new(AtomicU32::new(0));
  let received_sequence_clone = received_sequence.clone();

  registry.register::<TestPingRpc, _>(move |_world, rpc| {
    received_sequence_clone.store(rpc.payload.sequence, Ordering::SeqCst);
    Ok(())
  });

  // Client sends RPC
  let rpc = TestPingRpc { sequence: 12345 };
  let serialized = SerializedRpc::from_rpc(&rpc, client.client_id(), RpcTarget::Server).unwrap();
  client.send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());

  // Server processes RPC
  let client_ids: Vec<_> = server.clients().collect();
  let client_id = client_ids[0];
  let mut world = World::new();
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = server.receive(client_id, Channel::ReliableOrdered) {
      if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
        let _ = registry.dispatch(&mut world, &serialized);
      }
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert_eq!(
    received_sequence.load(Ordering::SeqCst),
    12345,
    "RPC handler was not called with correct sequence"
  );
}

#[test]
fn listen_server_replication_excludes_host_from_network() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88885,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88885)).unwrap();

  // Wait for connection
  let connected =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, client| {
      client.is_connected() && server.remote_count() == 1
    });
  assert!(connected);

  // Setup replication
  let mut world = World::new();
  let mut replication = ReplicationManager::new_server();
  replication.register::<Transform>();

  // Spawn entity for host
  let host_entity = replication.spawn_networked(
    &mut world,
    NetworkEntityBuilder::new().client_owned(server.host_id()),
  );
  world
    .entity_mut(host_entity)
    .insert(Transform::from_xy(100.0, 100.0));

  // Spawn entity for remote client
  let remote_clients: Vec<_> = server.remote_clients().collect();
  let remote_id = remote_clients[0];

  let remote_entity = replication.spawn_networked(
    &mut world,
    NetworkEntityBuilder::new().client_owned(remote_id),
  );
  world
    .entity_mut(remote_entity)
    .insert(Transform::from_xy(200.0, 200.0));

  // Verify both entities exist
  assert!(world.get::<Transform>(host_entity).is_some());
  assert!(world.get::<Transform>(remote_entity).is_some());

  // Verify host is correctly identified
  assert!(server.is_host(server.host_id()));
  assert!(!server.is_host(remote_id));

  // Verify player count
  assert_eq!(server.player_count(), 2); // Host + 1 remote
}

#[test]
fn serverlike_trait_works_with_network_server() {
  let port = get_test_port();

  let mut server = NetworkServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88886,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88886)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected);

  // Test ServerLike methods
  fn test_serverlike<S: ServerLike>(server: &mut S) {
    let clients: Vec<_> = server.clients().collect();
    assert_eq!(clients.len(), 1);

    // NetworkServer has no host
    assert!(!server.is_host(clients[0]));
  }

  test_serverlike(&mut server);
}

#[test]
fn serverlike_trait_works_with_listen_server() {
  let port = get_test_port();

  let mut server = ListenServer::new(NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 88887,
    ..Default::default()
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 88887)).unwrap();

  // Wait for connection
  let connected =
    run_listen_server_updates_until(&mut server, &mut client, 2000, |server, client| {
      client.is_connected() && server.remote_count() == 1
    });
  assert!(connected);

  // Test ServerLike methods
  fn test_serverlike<S: ServerLike>(server: &mut S, expected_host: bool) {
    let clients: Vec<_> = server.clients().collect();

    if expected_host {
      // ListenServer includes host in clients()
      assert!(!clients.is_empty());
      assert!(server.is_host(LOCAL_HOST_ID));
    }
  }

  test_serverlike(&mut server, true);
}

#[test]
fn multiple_entities_replicate_correctly() {
  let mut world = World::new();
  let mut manager = ReplicationManager::new_server();
  manager.register::<Transform>();
  manager.register::<Velocity>();

  // Spawn multiple entities
  let entities: Vec<_> = (0..10)
    .map(|i| {
      let entity = manager.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());
      world
        .entity_mut(entity)
        .insert(Transform::from_xy(i as f32 * 100.0, 0.0));
      world.entity_mut(entity).insert(Velocity::new(1.0, 0.0));
      entity
    })
    .collect();

  // Verify all entities were created with unique network IDs
  let network_ids: Vec<_> = entities
    .iter()
    .filter_map(|&e| manager.get_network_id(e))
    .collect();

  assert_eq!(network_ids.len(), 10);

  // Verify all IDs are unique
  let unique_ids: std::collections::HashSet<_> = network_ids.iter().collect();
  assert_eq!(unique_ids.len(), 10);

  // Verify bidirectional lookup works for all
  for (entity, network_id) in entities.iter().zip(network_ids.iter()) {
    assert_eq!(manager.get_entity(*network_id), Some(*entity));
  }
}

// =============================================================================
// Transport Layer Tests
// =============================================================================

#[test]
fn transport_config_defaults_to_udp() {
  let config = TransportConfig::default();
  assert!(matches!(config, TransportConfig::Udp));
}

#[test]
fn server_config_uses_default_udp_transport() {
  let config = NetworkServerConfig::default();
  assert!(matches!(config.transport, TransportConfig::Udp));
}

#[test]
fn server_with_explicit_udp_transport_works() {
  let port = get_test_port();

  let config = NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 90001,
    transport: TransportConfig::Udp,
  };

  let server = NetworkServer::new(config);
  assert!(server.is_ok());
  assert!(server.unwrap().is_running());
}

#[test]
fn server_and_client_communicate_with_udp_transport() {
  let port = get_test_port();

  // Create server with explicit UDP transport
  let server_config = NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 90002,
    transport: TransportConfig::Udp,
  };
  let mut server = NetworkServer::new(server_config).expect("Failed to create server");

  // Create client with UDP transport
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let client_config = NetworkClientConfig::udp(server_addr, 90002);
  let mut client = NetworkClient::connect(client_config).expect("Failed to create client");

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });

  assert!(connected, "UDP transport connection failed");
  assert!(client.is_connected());
  assert_eq!(server.connected_count(), 1);
}

#[test]
fn listen_server_uses_default_udp_transport() {
  let port = get_test_port();

  let config = NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 90003,
    transport: TransportConfig::Udp,
  };

  let server = ListenServer::new(config);
  assert!(server.is_ok());
  assert!(server.unwrap().is_running());
}

#[test]
fn udp_transport_handles_message_exchange() {
  let port = get_test_port();

  let server_config = NetworkServerConfig {
    port,
    max_clients: 4,
    protocol_id: 90004,
    transport: TransportConfig::Udp,
  };
  let mut server = NetworkServer::new(server_config).unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig::udp(server_addr, 90004)).unwrap();

  // Wait for connection
  let connected = run_updates_until(&mut server, &mut client, 2000, |server, client| {
    client.is_connected() && server.connected_count() == 1
  });
  assert!(connected);

  // Get client ID and send message
  let client_ids: Vec<_> = server.clients().collect();
  let client_id = client_ids[0];

  let message = b"UDP transport test message!";
  server.send(client_id, Channel::ReliableOrdered, message);

  // Wait for client to receive
  let mut received_message = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    if let Some(data) = client.receive(Channel::ReliableOrdered) {
      received_message = Some(data);
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(received_message.is_some(), "UDP message not received");
  assert_eq!(received_message.unwrap(), message.to_vec());
}

// WebRTC transport tests are feature-gated and require a running signaling server
// They are better suited for manual/E2E testing rather than CI
#[cfg(feature = "matchbox")]
mod webrtc_transport_tests {
  use super::*;

  #[test]
  fn webrtc_config_creates_correctly() {
    let config = WebRtcConfig::new("ws://localhost:3536", "test-room");

    assert_eq!(config.signaling_url, "ws://localhost:3536");
    assert_eq!(config.room_id, "test-room");
    assert!(!config.ice_servers.is_empty()); // Has default STUN servers
  }

  #[test]
  fn webrtc_config_with_turn_works() {
    let config = WebRtcConfig::new("ws://localhost:3536", "test-room").with_turn(
      "turn:relay.example.com:3478",
      "user",
      "pass",
    );

    let turn_servers: Vec<_> = config
      .ice_servers
      .iter()
      .filter(|s| s.url.starts_with("turn:"))
      .collect();

    assert_eq!(turn_servers.len(), 1);
    assert_eq!(turn_servers[0].username.as_deref(), Some("user"));
    assert_eq!(turn_servers[0].credential.as_deref(), Some("pass"));
  }

  #[test]
  fn transport_config_webrtc_variant_works() {
    let webrtc_config = WebRtcConfig::new("ws://localhost:3536", "room");
    let transport = TransportConfig::WebRtc(webrtc_config);

    assert!(matches!(transport, TransportConfig::WebRtc(_)));
  }

  #[test]
  fn server_config_accepts_webrtc_transport() {
    let webrtc_config = WebRtcConfig::new("ws://localhost:3536", "room");
    let config = NetworkServerConfig {
      port: 0, // Not used for WebRTC
      max_clients: 4,
      protocol_id: 12345,
      transport: TransportConfig::WebRtc(webrtc_config),
    };

    // Just verify config creation doesn't panic
    assert!(matches!(config.transport, TransportConfig::WebRtc(_)));
  }

  // Note: Full WebRTC integration tests require a running signaling server
  // and are better suited for E2E tests rather than unit/integration tests.
  // See tests/e2e/network.rs for WebRTC E2E tests.
}
