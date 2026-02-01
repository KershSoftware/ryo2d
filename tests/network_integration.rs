//! Integration tests for the networking module
//!
//! These tests verify actual network communication between server and client.

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
  };
  let mut server = NetworkServer::new(config).expect("Failed to create server");
  assert!(server.is_running());
  assert_eq!(server.connected_count(), 0);

  // Connect client
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let client_config = NetworkClientConfig {
    server_addr,
    protocol_id: 12345,
  };
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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 54321,
  })
  .unwrap();

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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 11111,
  })
  .unwrap();

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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 22222,
  })
  .unwrap();

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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 33333,
  })
  .unwrap();

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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

  // Connect two clients
  let mut client1 = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 44444,
  })
  .unwrap();

  let mut client2 = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 44444,
  })
  .unwrap();

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

  assert_eq!(server.connected_count(), 2, "Both clients should be connected");

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

    if client1_received.is_none() && let Some(data) = client1.receive(Channel::ReliableOrdered) {
      client1_received = Some(data);
    }

    if client2_received.is_none() && let Some(data) = client2.receive(Channel::ReliableOrdered) {
      client2_received = Some(data);
    }

    if client1_received.is_some() && client2_received.is_some() {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(client1_received.is_some(), "Client 1 did not receive broadcast");
  assert!(client2_received.is_some(), "Client 2 did not receive broadcast");
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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 55555,
  })
  .unwrap();

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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 11111, // Client uses different ID
  })
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
  })
  .unwrap();

  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = NetworkClient::connect(NetworkClientConfig {
    server_addr,
    protocol_id: 66666,
  })
  .unwrap();

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
