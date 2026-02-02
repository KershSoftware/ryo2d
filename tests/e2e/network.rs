//! End-to-end network tests for multi-client scenarios
//!
//! These tests validate full replication and RPC flows across multiple clients:
//! - Server ↔ Client A ↔ Client B communication
//! - Entity replication across all connected clients
//! - RPC routing and broadcast
//! - Late join state synchronization
//! - Disconnect handling
//!
//! All tests run headless (no window/GPU required).
//!
//! Run with: `cargo test --test network_e2e`

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::thread;
use std::time::Duration;

/// Atomic counter to ensure each test uses a unique port
static PORT_COUNTER: AtomicU16 = AtomicU16::new(16000);

fn get_test_port() -> u16 {
  PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

// =============================================================================
// Test Components and RPCs
// =============================================================================

/// Player position component for replication tests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PlayerPosition {
  x: f32,
  y: f32,
}

impl Replicated for PlayerPosition {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("PlayerPosition")
  }

  fn component_name() -> &'static str {
    "PlayerPosition"
  }

  fn replication_priority() -> u8 {
    10
  }
}

/// Player input RPC sent from client to server
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PlayerMoveRpc {
  dx: f32,
  dy: f32,
}

impl Rpc for PlayerMoveRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("PlayerMoveRpc")
  }

  fn rpc_name() -> &'static str {
    "PlayerMoveRpc"
  }
}

/// Chat message RPC for broadcast tests
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ChatMessageRpc {
  sender_name: String,
  message: String,
}

impl Rpc for ChatMessageRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("ChatMessageRpc")
  }

  fn rpc_name() -> &'static str {
    "ChatMessageRpc"
  }
}

/// Server-to-client game event RPC
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct GameEventRpc {
  event_type: String,
  data: String,
}

impl Rpc for GameEventRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("GameEventRpc")
  }

  fn rpc_name() -> &'static str {
    "GameEventRpc"
  }
}

// =============================================================================
// Test Harness: Headless Multi-Client Setup
// =============================================================================

/// Represents a headless client with its own World and ReplicationManager
struct HeadlessClient {
  client: NetworkClient,
  world: World,
  replication: ReplicationManager,
  /// Maps NetworkId to local Entity
  entity_map: HashMap<NetworkId, Entity>,
}

impl HeadlessClient {
  fn new(server_addr: SocketAddr, protocol_id: u64) -> Result<Self, NetworkError> {
    let client = NetworkClient::connect(NetworkClientConfig {
      server_addr,
      protocol_id,
    })?;

    let mut replication = ReplicationManager::new_client();
    replication.register::<Transform>();
    replication.register::<PlayerPosition>();

    Ok(Self {
      client,
      world: World::new(),
      replication,
      entity_map: HashMap::new(),
    })
  }

  fn update(&mut self, dt: Duration) {
    self.client.update(dt);
  }

  fn is_connected(&self) -> bool {
    self.client.is_connected()
  }

  fn client_id(&self) -> ClientId {
    self.client.client_id()
  }

  /// Process all incoming replication messages
  fn process_replication(&mut self) {
    // Process reliable channel (spawns, despawns, full state)
    while let Some(data) = self.client.receive(Channel::ReliableOrdered) {
      if let Ok(msg) = ReplicationMessage::from_bytes(&data) {
        self.handle_replication_message(msg);
      }
    }

    // Process unreliable channel (component updates)
    while let Some(data) = self.client.receive(Channel::Unreliable) {
      if let Ok(msg) = ReplicationMessage::from_bytes(&data) {
        self.handle_replication_message(msg);
      }
    }
  }

  fn handle_replication_message(&mut self, msg: ReplicationMessage) {
    match msg {
      ReplicationMessage::SpawnEntity(spawn) => {
        self.handle_spawn(spawn);
      }
      ReplicationMessage::DespawnEntity(despawn) => {
        self.handle_despawn(despawn);
      }
      ReplicationMessage::ComponentBatch(batch) => {
        for update in batch.updates {
          self.handle_component_update(update);
        }
      }
      ReplicationMessage::FullStateSync(sync) => {
        for spawn in sync.entities {
          self.handle_spawn(spawn);
        }
      }
      _ => {}
    }
  }

  fn handle_spawn(&mut self, spawn: SpawnEntityMessage) {
    // Create local entity with NetworkEntity component
    let entity = self.world.spawn((NetworkEntity {
      network_id: spawn.network_id,
      owner: spawn.owner,
      authority: spawn.authority,
    },));

    // Apply initial components
    for serialized in &spawn.components {
      self.apply_component(entity, serialized);
    }

    // Track mapping
    self.entity_map.insert(spawn.network_id, entity);
  }

  fn handle_despawn(&mut self, despawn: DespawnEntityMessage) {
    if let Some(entity) = self.entity_map.remove(&despawn.network_id) {
      let _ = self.world.despawn(entity);
    }
  }

  fn handle_component_update(&mut self, update: ComponentUpdateMessage) {
    if let Some(&entity) = self.entity_map.get(&update.network_id) {
      self.apply_component(entity, &update.component);
    }
  }

  fn apply_component(&mut self, entity: Entity, serialized: &SerializedComponent) {
    // Try to deserialize as known types
    if serialized.type_id == Transform::component_id() {
      if let Ok(transform) = serialized.clone().into_component::<Transform>() {
        if self.world.get_component::<Transform>(entity).is_some() {
          if let Some(mut t) = self.world.get_component_mut::<Transform>(entity) {
            *t = transform;
          }
        } else {
          self.world.add_component(entity, transform);
        }
      }
    } else if serialized.type_id == PlayerPosition::component_id() {
      if let Ok(pos) = serialized.clone().into_component::<PlayerPosition>() {
        if self.world.get_component::<PlayerPosition>(entity).is_some() {
          if let Some(mut p) = self.world.get_component_mut::<PlayerPosition>(entity) {
            *p = pos;
          }
        } else {
          self.world.add_component(entity, pos);
        }
      }
    }
  }

  /// Get entity by network ID
  fn get_entity(&self, network_id: NetworkId) -> Option<Entity> {
    self.entity_map.get(&network_id).copied()
  }

  /// Count of replicated entities
  fn entity_count(&self) -> usize {
    self.entity_map.len()
  }
}

/// Headless server with its own World and ReplicationManager
struct HeadlessServer {
  server: NetworkServer,
  world: World,
  replication: ReplicationManager,
  /// Maps ClientId to their player entity
  player_entities: HashMap<ClientId, Entity>,
}

impl HeadlessServer {
  fn new(port: u16, protocol_id: u64) -> Result<Self, NetworkError> {
    let server = NetworkServer::new(NetworkServerConfig {
      port,
      max_clients: 8,
      protocol_id,
    })?;

    let mut replication = ReplicationManager::new_server();
    replication.register::<Transform>();
    replication.register::<PlayerPosition>();

    Ok(Self {
      server,
      world: World::new(),
      replication,
      player_entities: HashMap::new(),
    })
  }

  fn update(&mut self, dt: Duration) {
    self.server.update(dt);
  }

  fn connected_count(&self) -> usize {
    self.server.connected_count()
  }

  fn clients(&self) -> impl Iterator<Item = ClientId> + '_ {
    self.server.clients()
  }

  /// Handle new connections and spawn player entities
  fn process_events(&mut self) {
    // Collect events first to avoid borrow issues
    let events: Vec<_> = self.server.events().collect();
    for event in events {
      match event {
        NetworkEvent::ClientConnected { client_id } => {
          self.on_client_connected(client_id);
        }
        NetworkEvent::ClientDisconnected { client_id, .. } => {
          self.on_client_disconnected(client_id);
        }
        _ => {}
      }
    }
  }

  fn on_client_connected(&mut self, client_id: ClientId) {
    // Spawn player entity for this client
    let entity = self.replication.spawn_networked(
      &mut self.world,
      NetworkEntityBuilder::new().client_owned(client_id),
    );
    self.world.add_component(entity, Transform::new(0.0, 0.0));
    self.world.add_component(
      entity,
      PlayerPosition {
        x: 100.0 * client_id as f32,
        y: 100.0,
      },
    );

    self.player_entities.insert(client_id, entity);

    // Send full state to new client
    self
      .replication
      .send_full_state(&self.world, &mut self.server, client_id);
  }

  fn on_client_disconnected(&mut self, client_id: ClientId) {
    if let Some(entity) = self.player_entities.remove(&client_id) {
      self.replication.despawn_networked(&mut self.world, entity);
    }
  }

  /// Broadcast all pending replication updates
  fn broadcast_updates(&mut self) {
    self
      .replication
      .broadcast_updates(&self.world, &mut self.server);
  }

  /// Get player entity for a client
  fn get_player_entity(&self, client_id: ClientId) -> Option<Entity> {
    self.player_entities.get(&client_id).copied()
  }

  /// Move a player and mark dirty
  fn move_player(&mut self, client_id: ClientId, dx: f32, dy: f32) {
    if let Some(&entity) = self.player_entities.get(&client_id) {
      if let Some(mut pos) = self.world.get_component_mut::<PlayerPosition>(entity) {
        pos.x += dx;
        pos.y += dy;
      }
      self.replication.mark_dirty::<PlayerPosition>(entity);
    }
  }
}

/// Run update loops until condition is met or timeout
fn run_e2e_until<F>(
  server: &mut HeadlessServer,
  clients: &mut [&mut HeadlessClient],
  timeout_ms: u64,
  mut condition: F,
) -> bool
where
  F: FnMut(&mut HeadlessServer, &mut [&mut HeadlessClient]) -> bool,
{
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(timeout_ms);

  while start.elapsed() < timeout {
    server.update(dt);
    for client in clients.iter_mut() {
      client.update(dt);
    }

    if condition(server, clients) {
      return true;
    }

    thread::sleep(Duration::from_millis(10));
  }

  false
}

// =============================================================================
// E2E Tests: Multi-Client Replication
// =============================================================================

#[test]
fn e2e_two_clients_both_receive_spawned_entities() {
  let port = get_test_port();
  let protocol_id = 100001;

  // Start server
  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");

  // Connect two clients
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");

  // Wait for both to connect
  let connected = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      clients[0].is_connected() && clients[1].is_connected() && server.connected_count() == 2
    },
  );
  assert!(connected, "Both clients should connect");

  // Process replication on clients
  let synced = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.broadcast_updates();
      clients[0].process_replication();
      clients[1].process_replication();

      // Both clients should have 2 entities (one for each player)
      clients[0].entity_count() == 2 && clients[1].entity_count() == 2
    },
  );

  assert!(synced, "Both clients should receive both player entities");
  assert_eq!(client_a.entity_count(), 2);
  assert_eq!(client_b.entity_count(), 2);
}

#[test]
fn e2e_client_a_movement_propagates_to_client_b() {
  let port = get_test_port();
  let protocol_id = 100002;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");

  // Connect and sync initial state
  let connected = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      server.broadcast_updates();
      clients[0].process_replication();
      clients[1].process_replication();

      clients[0].is_connected()
        && clients[1].is_connected()
        && clients[0].entity_count() == 2
        && clients[1].entity_count() == 2
    },
  );
  assert!(connected, "Initial sync should complete");

  // Get client A's ID and move their player on the server
  let client_a_id = client_a.client_id();
  let player_entity = server.get_player_entity(client_a_id).unwrap();
  let network_id = server.replication.get_network_id(player_entity).unwrap();

  // Move client A's player
  server.move_player(client_a_id, 50.0, 25.0);

  // Wait for update to propagate to client B
  let propagated = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.broadcast_updates();
      clients[0].process_replication();
      clients[1].process_replication();

      // Check if client B sees the updated position
      if let Some(entity) = clients[1].get_entity(network_id) {
        if let Some(pos) = clients[1].world.get_component::<PlayerPosition>(entity) {
          // Initial was 100.0 * client_id, moved by 50.0
          return pos.x > 100.0;
        }
      }
      false
    },
  );

  assert!(
    propagated,
    "Client A's movement should propagate to Client B"
  );

  // Verify the position on client B
  let entity_b = client_b.get_entity(network_id).unwrap();
  let pos_b = client_b
    .world
    .get_component::<PlayerPosition>(entity_b)
    .unwrap();
  assert!(pos_b.x >= 150.0, "Position should reflect the movement");
}

#[test]
fn e2e_late_join_receives_full_state() {
  let port = get_test_port();
  let protocol_id = 100003;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

  // Client A connects first
  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");

  let first_connected = run_e2e_until(
    &mut server,
    &mut [&mut client_a],
    3000,
    |server, clients| {
      server.process_events();
      server.broadcast_updates();
      clients[0].process_replication();

      clients[0].is_connected() && clients[0].entity_count() == 1
    },
  );
  assert!(
    first_connected,
    "Client A should connect and receive entity"
  );

  // Modify the entity state before client B joins
  let client_a_id = client_a.client_id();
  server.move_player(client_a_id, 999.0, 888.0);

  // Now client B joins late
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");

  let late_joined = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      server.broadcast_updates();
      clients[0].process_replication();
      clients[1].process_replication();

      // Client B should receive full state (2 entities with current positions)
      clients[1].is_connected() && clients[1].entity_count() == 2
    },
  );
  assert!(
    late_joined,
    "Client B should receive full state on late join"
  );

  // Verify client B has the updated position
  let player_entity = server.get_player_entity(client_a_id).unwrap();
  let network_id = server.replication.get_network_id(player_entity).unwrap();

  if let Some(entity) = client_b.get_entity(network_id) {
    let pos = client_b
      .world
      .get_component::<PlayerPosition>(entity)
      .unwrap();
    // Position should include the movement (initial was 100*id, moved by 999)
    assert!(
      pos.x > 500.0,
      "Late joiner should see updated position, got {}",
      pos.x
    );
  } else {
    panic!("Client B should have client A's player entity");
  }
}

#[test]
fn e2e_client_disconnect_removes_entity_from_others() {
  let port = get_test_port();
  let protocol_id = 100004;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");

  // Connect both and sync
  let synced = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      server.broadcast_updates();
      clients[0].process_replication();
      clients[1].process_replication();

      clients[0].entity_count() == 2 && clients[1].entity_count() == 2
    },
  );
  assert!(synced, "Initial sync should complete");

  // Client A disconnects
  client_a.client.disconnect();

  // Wait for server to process disconnect and notify client B
  let cleaned_up = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      server.broadcast_updates();
      clients[1].process_replication();

      // Client B should now only have 1 entity (their own)
      server.connected_count() == 1 && clients[1].entity_count() == 1
    },
  );

  assert!(
    cleaned_up,
    "Client B should have only 1 entity after A disconnects"
  );
  assert_eq!(client_b.entity_count(), 1);
}

// =============================================================================
// E2E Tests: RPC Routing
// =============================================================================

#[test]
fn e2e_rpc_client_to_server() {
  let port = get_test_port();
  let protocol_id = 100010;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client");

  // Connect
  let connected = run_e2e_until(&mut server, &mut [&mut client], 3000, |server, clients| {
    server.process_events();
    clients[0].is_connected()
  });
  assert!(connected);

  let client_id = client.client_id();

  // Client sends move RPC to server
  let move_rpc = PlayerMoveRpc { dx: 10.0, dy: 5.0 };
  let serialized = SerializedRpc::from_rpc(&move_rpc, client_id, RpcTarget::Server).unwrap();
  client
    .client
    .send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());

  // Server receives and processes
  let mut received_rpc = None;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client.update(dt);

    for cid in server.clients().collect::<Vec<_>>() {
      if let Some(data) = server.server.receive(cid, Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
          if let Ok(rpc) = serialized.into_rpc::<PlayerMoveRpc>() {
            received_rpc = Some(rpc);
            break;
          }
        }
      }
    }

    if received_rpc.is_some() {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(received_rpc.is_some(), "Server should receive RPC");
  let rpc = received_rpc.unwrap();
  assert_eq!(rpc.payload.dx, 10.0);
  assert_eq!(rpc.payload.dy, 5.0);
  assert_eq!(rpc.sender, client_id);
}

#[test]
fn e2e_rpc_server_broadcast_to_all_clients() {
  let port = get_test_port();
  let protocol_id = 100011;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");

  // Connect both
  let connected = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      clients[0].is_connected() && clients[1].is_connected()
    },
  );
  assert!(connected);

  // Server broadcasts game event
  let event = GameEventRpc {
    event_type: "ROUND_START".to_string(),
    data: "Round 1".to_string(),
  };
  let serialized = SerializedRpc::from_rpc(&event, 0, RpcTarget::Broadcast).unwrap();
  server
    .server
    .broadcast(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());

  // Both clients should receive
  let mut client_a_received = false;
  let mut client_b_received = false;
  let dt = Duration::from_millis(16);

  for _ in 0..100 {
    server.update(dt);
    client_a.update(dt);
    client_b.update(dt);

    if !client_a_received {
      if let Some(data) = client_a.client.receive(Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
          if let Ok(rpc) = serialized.into_rpc::<GameEventRpc>() {
            assert_eq!(rpc.payload.event_type, "ROUND_START");
            client_a_received = true;
          }
        }
      }
    }

    if !client_b_received {
      if let Some(data) = client_b.client.receive(Channel::ReliableOrdered) {
        if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
          if let Ok(rpc) = serialized.into_rpc::<GameEventRpc>() {
            assert_eq!(rpc.payload.event_type, "ROUND_START");
            client_b_received = true;
          }
        }
      }
    }

    if client_a_received && client_b_received {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(client_a_received, "Client A should receive broadcast");
  assert!(client_b_received, "Client B should receive broadcast");
}

#[test]
fn e2e_rpc_client_a_chat_relayed_to_client_b() {
  let port = get_test_port();
  let protocol_id = 100012;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");

  // Connect both
  let connected = run_e2e_until(
    &mut server,
    &mut [&mut client_a, &mut client_b],
    3000,
    |server, clients| {
      server.process_events();
      clients[0].is_connected() && clients[1].is_connected()
    },
  );
  assert!(connected);

  let client_a_id = client_a.client_id();

  // Client A sends chat message to server
  let chat = ChatMessageRpc {
    sender_name: "PlayerA".to_string(),
    message: "Hello everyone!".to_string(),
  };
  let serialized = SerializedRpc::from_rpc(&chat, client_a_id, RpcTarget::Server).unwrap();
  client_a
    .client
    .send(Channel::ReliableOrdered, &serialized.to_bytes().unwrap());

  // Server receives and broadcasts to all
  let dt = Duration::from_millis(16);
  let mut server_received = false;

  for _ in 0..50 {
    server.update(dt);
    client_a.update(dt);
    client_b.update(dt);

    for cid in server.clients().collect::<Vec<_>>() {
      if let Some(data) = server.server.receive(cid, Channel::ReliableOrdered) {
        if SerializedRpc::from_bytes(&data).is_ok() {
          // Re-broadcast to all clients
          let broadcast =
            SerializedRpc::from_rpc(&chat, client_a_id, RpcTarget::Broadcast).unwrap();
          server
            .server
            .broadcast(Channel::ReliableOrdered, &broadcast.to_bytes().unwrap());
          server_received = true;
          break;
        }
      }
    }

    if server_received {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(server_received, "Server should receive chat from A");

  // Client B should receive the relayed chat
  let mut client_b_received = false;

  for _ in 0..50 {
    server.update(dt);
    client_a.update(dt);
    client_b.update(dt);

    if let Some(data) = client_b.client.receive(Channel::ReliableOrdered) {
      if let Ok(serialized) = SerializedRpc::from_bytes(&data) {
        if let Ok(rpc) = serialized.into_rpc::<ChatMessageRpc>() {
          assert_eq!(rpc.payload.sender_name, "PlayerA");
          assert_eq!(rpc.payload.message, "Hello everyone!");
          assert_eq!(rpc.sender, client_a_id);
          client_b_received = true;
          break;
        }
      }
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(
    client_b_received,
    "Client B should receive chat from A via server"
  );
}

// =============================================================================
// E2E Tests: Stress and Edge Cases
// =============================================================================

#[test]
fn e2e_multiple_rapid_updates_arrive_in_order() {
  let port = get_test_port();
  let protocol_id = 100020;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client");

  // Connect and sync
  let connected = run_e2e_until(&mut server, &mut [&mut client], 3000, |server, clients| {
    server.process_events();
    server.broadcast_updates();
    clients[0].process_replication();

    clients[0].is_connected() && clients[0].entity_count() == 1
  });
  assert!(connected);

  let client_id = client.client_id();

  // Make 10 rapid position updates
  for _ in 0..10 {
    server.move_player(client_id, 10.0, 0.0);
  }

  // Broadcast and process
  let final_synced = run_e2e_until(&mut server, &mut [&mut client], 3000, |server, clients| {
    server.broadcast_updates();
    clients[0].process_replication();

    // Check final position
    let player_entity = server.get_player_entity(client_id).unwrap();
    let network_id = server.replication.get_network_id(player_entity).unwrap();

    if let Some(entity) = clients[0].get_entity(network_id) {
      if let Some(pos) = clients[0].world.get_component::<PlayerPosition>(entity) {
        // Initial was 100*id + 100, moved 10*10 = 100
        return pos.x >= 200.0;
      }
    }
    false
  });

  assert!(final_synced, "All updates should arrive");
}

#[test]
fn e2e_three_clients_all_see_each_other() {
  let port = get_test_port();
  let protocol_id = 100021;

  let mut server = HeadlessServer::new(port, protocol_id).expect("Failed to create server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

  let mut client_a =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client A");
  let mut client_b =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client B");
  let mut client_c =
    HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client C");

  // Connect all three
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(5000);

  while start.elapsed() < timeout {
    server.update(dt);
    server.process_events();
    client_a.update(dt);
    client_b.update(dt);
    client_c.update(dt);

    if client_a.is_connected() && client_b.is_connected() && client_c.is_connected() {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(client_a.is_connected());
  assert!(client_b.is_connected());
  assert!(client_c.is_connected());
  assert_eq!(server.connected_count(), 3);

  // Sync all entities
  let all_synced = {
    let start = std::time::Instant::now();
    let mut synced = false;

    while start.elapsed() < timeout {
      server.update(dt);
      server.broadcast_updates();
      client_a.update(dt);
      client_a.process_replication();
      client_b.update(dt);
      client_b.process_replication();
      client_c.update(dt);
      client_c.process_replication();

      if client_a.entity_count() == 3
        && client_b.entity_count() == 3
        && client_c.entity_count() == 3
      {
        synced = true;
        break;
      }

      thread::sleep(Duration::from_millis(10));
    }

    synced
  };

  assert!(all_synced, "All 3 clients should see all 3 player entities");
  assert_eq!(client_a.entity_count(), 3);
  assert_eq!(client_b.entity_count(), 3);
  assert_eq!(client_c.entity_count(), 3);
}

// =============================================================================
// E2E Tests: ListenServer Mode
// =============================================================================

/// Headless listen server (host + remotes)
struct HeadlessListenServer {
  server: ListenServer,
  world: World,
  replication: ReplicationManager,
  player_entities: HashMap<ClientId, Entity>,
}

impl HeadlessListenServer {
  fn new(port: u16, protocol_id: u64) -> Result<Self, NetworkError> {
    let server = ListenServer::new(NetworkServerConfig {
      port,
      max_clients: 8,
      protocol_id,
    })?;

    let mut replication = ReplicationManager::new_server();
    replication.register::<Transform>();
    replication.register::<PlayerPosition>();

    let mut this = Self {
      server,
      world: World::new(),
      replication,
      player_entities: HashMap::new(),
    };

    // Spawn host player immediately
    this.spawn_player(LOCAL_HOST_ID);

    Ok(this)
  }

  fn update(&mut self, dt: Duration) {
    self.server.update(dt);
  }

  fn remote_count(&self) -> usize {
    self.server.remote_count()
  }

  fn process_events(&mut self) {
    // Collect events first to avoid borrow issues
    let events: Vec<_> = self.server.events().collect();
    for event in events {
      match event {
        NetworkEvent::ClientConnected { client_id } => {
          self.spawn_player(client_id);
          self
            .replication
            .send_full_state(&self.world, &mut self.server, client_id);
        }
        NetworkEvent::ClientDisconnected { client_id, .. } => {
          if let Some(entity) = self.player_entities.remove(&client_id) {
            self.replication.despawn_networked(&mut self.world, entity);
          }
        }
        _ => {}
      }
    }
  }

  fn spawn_player(&mut self, client_id: ClientId) {
    let entity = self.replication.spawn_networked(
      &mut self.world,
      NetworkEntityBuilder::new().client_owned(client_id),
    );
    self.world.add_component(entity, Transform::new(0.0, 0.0));
    self.world.add_component(
      entity,
      PlayerPosition {
        x: 50.0 * client_id as f32,
        y: 50.0,
      },
    );
    self.player_entities.insert(client_id, entity);
  }

  fn broadcast_updates(&mut self) {
    self
      .replication
      .broadcast_updates(&self.world, &mut self.server);
  }

  fn player_count(&self) -> usize {
    self.server.player_count()
  }

  fn get_player_entity(&self, client_id: ClientId) -> Option<Entity> {
    self.player_entities.get(&client_id).copied()
  }

  fn move_player(&mut self, client_id: ClientId, dx: f32, dy: f32) {
    if let Some(&entity) = self.player_entities.get(&client_id) {
      if let Some(mut pos) = self.world.get_component_mut::<PlayerPosition>(entity) {
        pos.x += dx;
        pos.y += dy;
      }
      self.replication.mark_dirty::<PlayerPosition>(entity);
    }
  }
}

#[test]
fn e2e_listen_server_host_and_remote_both_have_entities() {
  let port = get_test_port();
  let protocol_id = 100030;

  let mut server =
    HeadlessListenServer::new(port, protocol_id).expect("Failed to create listen server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client");

  // Connect remote
  let dt = Duration::from_millis(16);
  let start = std::time::Instant::now();
  let timeout = Duration::from_millis(3000);

  while start.elapsed() < timeout {
    server.update(dt);
    server.process_events();
    client.update(dt);

    if client.is_connected() && server.remote_count() == 1 {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert!(client.is_connected());
  assert_eq!(server.player_count(), 2); // Host + 1 remote

  // Sync entities to client
  let synced = {
    let start = std::time::Instant::now();
    let mut result = false;

    while start.elapsed() < timeout {
      server.update(dt);
      server.broadcast_updates();
      client.update(dt);
      client.process_replication();

      // Client should see both host and self
      if client.entity_count() == 2 {
        result = true;
        break;
      }

      thread::sleep(Duration::from_millis(10));
    }

    result
  };

  assert!(
    synced,
    "Remote client should see both host and self entities"
  );
  assert_eq!(client.entity_count(), 2);
}

#[test]
fn e2e_listen_server_host_movement_reaches_remote() {
  let port = get_test_port();
  let protocol_id = 100031;

  let mut server =
    HeadlessListenServer::new(port, protocol_id).expect("Failed to create listen server");
  let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
  let mut client = HeadlessClient::new(server_addr, protocol_id).expect("Failed to create client");

  // Connect and sync
  let dt = Duration::from_millis(16);
  let timeout = Duration::from_millis(3000);
  let start = std::time::Instant::now();

  while start.elapsed() < timeout {
    server.update(dt);
    server.process_events();
    server.broadcast_updates();
    client.update(dt);
    client.process_replication();

    if client.is_connected() && client.entity_count() == 2 {
      break;
    }

    thread::sleep(Duration::from_millis(10));
  }

  assert_eq!(client.entity_count(), 2);

  // Move host player
  server.move_player(LOCAL_HOST_ID, 500.0, 250.0);

  // Check propagation
  let host_entity = server.get_player_entity(LOCAL_HOST_ID).unwrap();
  let host_network_id = server.replication.get_network_id(host_entity).unwrap();

  let propagated = {
    let start = std::time::Instant::now();
    let mut result = false;

    while start.elapsed() < timeout {
      server.update(dt);
      server.broadcast_updates();
      client.update(dt);
      client.process_replication();

      if let Some(entity) = client.get_entity(host_network_id) {
        if let Some(pos) = client.world.get_component::<PlayerPosition>(entity) {
          // Host started at 50*0=0, moved by 500
          if pos.x >= 400.0 {
            result = true;
            break;
          }
        }
      }

      thread::sleep(Duration::from_millis(10));
    }

    result
  };

  assert!(propagated, "Host movement should reach remote client");
}
