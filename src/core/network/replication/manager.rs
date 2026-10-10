//! Replication manager for coordinating network entity synchronization
//!
//! The `ReplicationManager` is the central hub for all replication operations,
//! handling entity spawning, despawning, component updates, and full state sync.

use std::any::TypeId;
use std::collections::HashMap;

use hecs::Entity;
use renet::ClientId;

use super::builder::NetworkEntityBuilder;
use super::component::{ComponentRegistry, ComponentTypeId, Replicated, SerializedComponent};
use super::dirty::DirtyTracker;
use super::messages::{
  AckMessage, ComponentBatchMessage, ComponentUpdateMessage, DespawnEntityMessage,
  FullStateSyncMessage, ReplicationMessage, SpawnEntityMessage,
};
use super::types::{NetworkEntity, NetworkId};
use crate::core::ecs::World;
use crate::core::error::ReplicationResult;
use crate::core::network::{Channel, ClientLike, ServerLike};

/// Role of this replication manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplicationRole {
  /// Server: authoritative, spawns entities, broadcasts updates
  Server,
  /// Client: receives updates, applies them to local world
  Client,
}

/// The replication manager coordinates all network entity synchronization.
///
/// # Server Usage
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// let mut replication = ReplicationManager::new_server();
/// let mut world = World::new();
/// replication.register::<Transform>();
///
/// // Spawn networked entity
/// let entity = replication.spawn_networked(
///     &mut world,
///     NetworkEntityBuilder::new()
///         .with_component(Transform::new(0.0, 0.0))
/// );
///
/// // Mark dirty after modification
/// replication.mark_dirty::<Transform>(entity);
///
/// // Broadcast to clients (requires a server instance)
/// // replication.broadcast_updates(&mut world, &mut server);
/// ```
///
/// # Client Usage
///
/// ```ignore
/// use ryo2d::prelude::*;
///
/// let mut replication = ReplicationManager::new_client();
/// replication.register::<Transform>();
///
/// // Process incoming messages (requires client instance)
/// // replication.process_incoming(&mut world, &mut client);
/// ```
/// Default number of `broadcast_updates` calls between full refreshes
/// (about five seconds at 60 updates per second - a safety net, as clients
/// acknowledge changes).
pub const DEFAULT_REFRESH_INTERVAL: u64 = 300;

/// Most component updates per refresh message, keeping messages small.
pub const REFRESH_BATCH_SIZE: usize = 64;

pub struct ReplicationManager {
  /// Server or client role
  role: ReplicationRole,

  /// Component type registry
  registry: ComponentRegistry,

  /// Dirty tracking for delta updates
  dirty_tracker: DirtyTracker,

  /// Maps NetworkId to local Entity
  network_to_entity: HashMap<NetworkId, Entity>,

  /// Maps local Entity to NetworkId
  entity_to_network: HashMap<Entity, NetworkId>,

  /// Registered component TypeIds for iteration
  registered_types: Vec<TypeId>,

  /// `broadcast_updates` calls between full refreshes; 0 disables them
  refresh_interval: u64,

  /// Client: generation of the newest change batch applied
  newest_batch_generation: Option<u64>,
}

impl ReplicationManager {
  /// Create a new server-side replication manager.
  pub fn new_server() -> Self {
    Self {
      role: ReplicationRole::Server,
      registry: ComponentRegistry::new(),
      dirty_tracker: DirtyTracker::new(),
      network_to_entity: HashMap::new(),
      entity_to_network: HashMap::new(),
      registered_types: Vec::new(),
      refresh_interval: DEFAULT_REFRESH_INTERVAL,
      newest_batch_generation: None,
    }
  }

  /// Create a new client-side replication manager.
  pub fn new_client() -> Self {
    Self {
      role: ReplicationRole::Client,
      registry: ComponentRegistry::new(),
      dirty_tracker: DirtyTracker::new(),
      network_to_entity: HashMap::new(),
      entity_to_network: HashMap::new(),
      registered_types: Vec::new(),
      refresh_interval: DEFAULT_REFRESH_INTERVAL,
      newest_batch_generation: None,
    }
  }

  /// Get the replication role.
  pub fn role(&self) -> ReplicationRole {
    self.role
  }

  /// Check if this is a server.
  pub fn is_server(&self) -> bool {
    self.role == ReplicationRole::Server
  }

  /// Check if this is a client.
  pub fn is_client(&self) -> bool {
    self.role == ReplicationRole::Client
  }

  /// Register a replicated component type.
  ///
  /// Both server and client must register the same component types
  /// for replication to work correctly.
  pub fn register<T: Replicated + 'static>(&mut self) {
    self.registry.register::<T>();
    self.registered_types.push(TypeId::of::<T>());
  }

  /// Get the component registry.
  pub fn registry(&self) -> &ComponentRegistry {
    &self.registry
  }

  /// Get the dirty tracker.
  pub fn dirty_tracker(&self) -> &DirtyTracker {
    &self.dirty_tracker
  }

  /// Get the dirty tracker mutably.
  pub fn dirty_tracker_mut(&mut self) -> &mut DirtyTracker {
    &mut self.dirty_tracker
  }

  // ========== Entity Management ==========

  /// Spawn a networked entity (server only).
  ///
  /// Creates a local entity with the specified components and registers
  /// it for network replication.
  pub fn spawn_networked(&mut self, world: &mut World, builder: NetworkEntityBuilder) -> Entity {
    let network_entity = builder.build_network_entity();
    let network_id = network_entity.network_id;

    let entity = world.spawn((network_entity,));
    builder.insert_components(world, entity);

    // Register the mapping
    self.network_to_entity.insert(network_id, entity);
    self.entity_to_network.insert(entity, network_id);

    // Mark as spawned for broadcast
    self.dirty_tracker.mark_spawned(network_id);

    entity
  }

  /// Spawn a networked entity from a spawn message (client).
  pub fn spawn_from_message(
    &mut self,
    world: &mut World,
    msg: &SpawnEntityMessage,
  ) -> ReplicationResult<Entity> {
    let network_entity = NetworkEntity {
      network_id: msg.network_id,
      owner: msg.owner,
      authority: msg.authority,
    };

    // Spawn with NetworkEntity component
    let entity = world.spawn((network_entity,));

    // Apply each serialized component
    for serialized in &msg.components {
      self
        .registry
        .deserialize_and_apply(serialized, world, entity)?;
    }

    // Register mapping
    self.network_to_entity.insert(msg.network_id, entity);
    self.entity_to_network.insert(entity, msg.network_id);

    Ok(entity)
  }

  /// Despawn a networked entity (server only).
  pub fn despawn_networked(&mut self, world: &mut World, entity: Entity) -> Option<NetworkId> {
    let network_id = self.entity_to_network.remove(&entity)?;
    self.network_to_entity.remove(&network_id);

    // Mark for broadcast before despawning
    self.dirty_tracker.mark_despawned(network_id);

    let _ = world.despawn(entity);

    Some(network_id)
  }

  /// Despawn an entity by network ID.
  pub fn despawn_by_network_id(&mut self, world: &mut World, network_id: NetworkId) -> bool {
    if let Some(entity) = self.network_to_entity.remove(&network_id) {
      self.entity_to_network.remove(&entity);
      let _ = world.despawn(entity);
      true
    } else {
      false
    }
  }

  /// Get the local entity for a network ID.
  pub fn get_entity(&self, network_id: NetworkId) -> Option<Entity> {
    self.network_to_entity.get(&network_id).copied()
  }

  /// Get the network ID for a local entity.
  pub fn get_network_id(&self, entity: Entity) -> Option<NetworkId> {
    self.entity_to_network.get(&entity).copied()
  }

  /// Check if an entity is networked.
  pub fn is_networked(&self, entity: Entity) -> bool {
    self.entity_to_network.contains_key(&entity)
  }

  // ========== Dirty Tracking ==========

  /// Mark a component as dirty for an entity.
  pub fn mark_dirty<T: Replicated>(&mut self, entity: Entity) {
    if let Some(network_id) = self.entity_to_network.get(&entity) {
      self
        .dirty_tracker
        .mark_dirty(*network_id, T::component_id());
    }
  }

  /// Mark a component dirty by network ID.
  pub fn mark_dirty_by_id(&mut self, network_id: NetworkId, component_id: ComponentTypeId) {
    self.dirty_tracker.mark_dirty(network_id, component_id);
  }

  // ========== Client Management ==========

  /// Register a new client (server only).
  pub fn register_client(&mut self, client_id: ClientId) {
    self.dirty_tracker.register_client(client_id);
  }

  /// The newest change batch generation a client has acknowledged.
  pub fn client_acked_generation(&self, client_id: ClientId) -> Option<u64> {
    self.dirty_tracker.client_generation(client_id)
  }

  /// Remove a disconnected client.
  pub fn remove_client(&mut self, client_id: ClientId) {
    self.dirty_tracker.remove_client(client_id);
  }

  // ========== Server: Sending Updates ==========

  /// Collect dirty component updates for a client.
  pub fn collect_dirty_updates(
    &self,
    world: &World,
    client_id: ClientId,
  ) -> Vec<ComponentUpdateMessage> {
    let mut updates = Vec::new();

    for (network_id, dirty_components) in self.dirty_tracker.get_dirty_for_client(client_id) {
      let Some(entity) = self.network_to_entity.get(&network_id) else {
        continue;
      };

      for component_id in dirty_components {
        // Try to serialize each dirty component
        if let Some(serialized) = self.serialize_component(world, *entity, component_id) {
          updates.push(ComponentUpdateMessage::new(network_id, serialized));
        }
      }
    }

    updates
  }

  /// Serialize a specific component from an entity.
  fn serialize_component(
    &self,
    world: &World,
    entity: Entity,
    component_id: ComponentTypeId,
  ) -> Option<SerializedComponent> {
    self
      .registry
      .serialize_from_entity(component_id, world, entity)
  }

  /// The spawn message for a networked entity, carrying the current state of
  /// every registered component it has, or `None` if it isn't networked.
  ///
  /// Spawns travel on the reliable channel, so a client never sees an entity
  /// without its initial state.
  pub fn spawn_message(&self, world: &World, network_id: NetworkId) -> Option<SpawnEntityMessage> {
    let entity = *self.network_to_entity.get(&network_id)?;
    let network_entity = world.get::<NetworkEntity>(entity)?;
    let mut msg =
      SpawnEntityMessage::new(network_id, network_entity.owner, network_entity.authority);
    msg.components = self
      .registry
      .registered_types()
      .filter_map(|component_id| {
        self
          .registry
          .serialize_from_entity(component_id, world, entity)
      })
      .collect();
    Some(msg)
  }

  /// The current value of every registered component on every networked entity.
  fn collect_all_updates(&self, world: &World) -> Vec<ComponentUpdateMessage> {
    let mut updates = Vec::new();
    for (&network_id, &entity) in &self.network_to_entity {
      for component_id in self.registry.registered_types() {
        if let Some(serialized) = self
          .registry
          .serialize_from_entity(component_id, world, entity)
        {
          updates.push(ComponentUpdateMessage::new(network_id, serialized));
        }
      }
    }
    updates
  }

  /// Build a full state sync message for a new client.
  pub fn build_full_state_sync(&self, world: &World) -> FullStateSyncMessage {
    let mut sync = FullStateSyncMessage::new(self.dirty_tracker.generation());

    for &network_id in self.network_to_entity.keys() {
      if let Some(spawn_msg) = self.spawn_message(world, network_id) {
        sync.add_entity(spawn_msg);
      }
    }

    sync
  }

  /// Send full state to a newly connected client.
  pub fn send_full_state<S: ServerLike>(&self, world: &World, server: &mut S, client_id: ClientId) {
    let sync = self.build_full_state_sync(world);
    let msg = ReplicationMessage::FullStateSync(sync);

    if let Ok(bytes) = msg.to_bytes() {
      server.send(client_id, Channel::ReplicationReliable, &bytes);
    }
  }

  /// Re-send every replicated component's current value every `interval`
  /// calls to [`broadcast_updates`](Self::broadcast_updates); `0` disables it.
  ///
  /// Deltas go out on the unreliable channel and aren't re-sent when a packet
  /// is lost, so a periodic refresh bounds how long a client can show a stale
  /// value. Default: [`DEFAULT_REFRESH_INTERVAL`].
  pub fn set_refresh_interval(&mut self, interval: u64) {
    self.refresh_interval = interval;
  }

  /// Broadcast all pending updates to clients.
  ///
  /// Spawns and despawns go out once on the reliable replication channel.
  /// Component changes go on the unreliable one and are re-sent every call
  /// until the client acknowledges a batch that covers them; acknowledgements
  /// are read from the same channel at the start of each call.
  pub fn broadcast_updates<S: ServerLike>(&mut self, world: &World, server: &mut S) {
    // Collect client IDs first to avoid borrow issues
    let clients: Vec<ClientId> = server.clients().collect();
    let remote_clients: Vec<ClientId> = clients
      .iter()
      .filter(|&&id| !server.is_host(id))
      .copied()
      .collect();

    // Record what each client has applied before deciding what to re-send
    for &client_id in &remote_clients {
      while let Some(data) = server.receive(client_id, Channel::ReplicationUnreliable) {
        if let Ok(ReplicationMessage::Ack(ack)) = ReplicationMessage::from_bytes(&data) {
          self.dirty_tracker.record_ack(client_id, ack.generation);
        }
      }
    }

    // Send spawn messages
    for network_id in self.dirty_tracker.spawned_entities() {
      if let Some(spawn_msg) = self.spawn_message(world, network_id) {
        let msg = ReplicationMessage::SpawnEntity(spawn_msg);

        if let Ok(bytes) = msg.to_bytes() {
          for &client_id in &remote_clients {
            server.send(client_id, Channel::ReplicationReliable, &bytes);
          }
        }
      }
    }

    // Send despawn messages
    for network_id in self.dirty_tracker.despawned_entities() {
      let msg = ReplicationMessage::DespawnEntity(DespawnEntityMessage::new(network_id));
      if let Ok(bytes) = msg.to_bytes() {
        for &client_id in &remote_clients {
          server.send(client_id, Channel::ReplicationReliable, &bytes);
        }
      }
    }

    // Send component updates per client
    for &client_id in &remote_clients {
      let updates = self.collect_dirty_updates(world, client_id);
      if !updates.is_empty() {
        let batch = ComponentBatchMessage {
          updates,
          generation: self.dirty_tracker.generation(),
        };
        let msg = ReplicationMessage::ComponentBatch(batch);

        if let Ok(bytes) = msg.to_bytes() {
          server.send(client_id, Channel::ReplicationUnreliable, &bytes);
        }
      }
    }

    // Periodic refresh: bounds how long a lost delta can leave a client stale
    let generation = self.dirty_tracker.generation();
    if self.refresh_interval > 0
      && generation > 0
      && generation.is_multiple_of(self.refresh_interval)
      && !remote_clients.is_empty()
    {
      let updates = self.collect_all_updates(world);
      for chunk in updates.chunks(REFRESH_BATCH_SIZE) {
        let msg = ReplicationMessage::ComponentRefresh(ComponentBatchMessage {
          updates: chunk.to_vec(),
          generation,
        });
        if let Ok(bytes) = msg.to_bytes() {
          for &client_id in &remote_clients {
            server.send(client_id, Channel::ReplicationUnreliable, &bytes);
          }
        }
      }
    }

    // Advance generation
    self.dirty_tracker.next_generation();
  }

  // ========== Client: Receiving Updates ==========

  /// Process incoming replication messages (client), then acknowledge the newest
  /// change batch applied so the server stops re-sending it.
  pub fn process_incoming<C: ClientLike>(&mut self, world: &mut World, client: &mut C) {
    while let Some(data) = client.receive(Channel::ReplicationReliable) {
      if let Ok(msg) = ReplicationMessage::from_bytes(&data) {
        self.handle_message(world, msg);
      }
    }

    let newest_before = self.newest_batch_generation;
    while let Some(data) = client.receive(Channel::ReplicationUnreliable) {
      if let Ok(msg) = ReplicationMessage::from_bytes(&data) {
        self.handle_message(world, msg);
      }
    }

    if let Some(generation) = self.newest_batch_generation
      && Some(generation) != newest_before
      && let Ok(bytes) = ReplicationMessage::Ack(AckMessage { generation }).to_bytes()
    {
      client.send(Channel::ReplicationUnreliable, &bytes);
    }
  }

  /// Whether a batch is at least as new as the newest change batch applied.
  fn is_current(&self, generation: u64) -> bool {
    self
      .newest_batch_generation
      .is_none_or(|newest| generation >= newest)
  }

  /// Handle a single replication message.
  fn handle_message(&mut self, world: &mut World, msg: ReplicationMessage) {
    match msg {
      ReplicationMessage::SpawnEntity(spawn) => {
        let _ = self.spawn_from_message(world, &spawn);
      }
      ReplicationMessage::DespawnEntity(despawn) => {
        self.despawn_by_network_id(world, despawn.network_id);
      }
      ReplicationMessage::ComponentUpdate(update) => {
        self.apply_component_update(world, &update);
      }
      ReplicationMessage::ComponentBatch(batch) => {
        if self.is_current(batch.generation) {
          for update in &batch.updates {
            self.apply_component_update(world, update);
          }
          self.newest_batch_generation = Some(batch.generation);
        }
      }
      ReplicationMessage::ComponentRefresh(batch) => {
        if self.is_current(batch.generation) {
          for update in &batch.updates {
            self.apply_component_update(world, update);
          }
        }
      }
      ReplicationMessage::Ack(_) => {
        // Client-to-server only; the server reads acks in broadcast_updates
      }
      ReplicationMessage::FullStateSync(sync) => {
        // Clear existing networked entities
        let to_remove: Vec<_> = self.network_to_entity.keys().copied().collect();
        for network_id in to_remove {
          self.despawn_by_network_id(world, network_id);
        }

        // Spawn all entities from sync
        for spawn in &sync.entities {
          let _ = self.spawn_from_message(world, spawn);
        }
      }
      ReplicationMessage::Rpc(_rpc) => {
        // RPC handling is done separately via RpcRegistry
      }
    }
  }

  /// Apply a component update to an entity.
  fn apply_component_update(&mut self, world: &mut World, update: &ComponentUpdateMessage) {
    let Some(&entity) = self.network_to_entity.get(&update.network_id) else {
      return;
    };

    let _ = self
      .registry
      .deserialize_and_apply(&update.component, world, entity);
  }

  /// Get stats about the replication state.
  pub fn stats(&self) -> ReplicationStats {
    ReplicationStats {
      networked_entities: self.network_to_entity.len(),
      registered_components: self.registry.registered_types().count(),
      generation: self.dirty_tracker.generation(),
    }
  }
}

/// Statistics about the replication manager state.
#[derive(Debug, Clone, Copy)]
pub struct ReplicationStats {
  /// Number of networked entities
  pub networked_entities: usize,
  /// Number of registered component types
  pub registered_components: usize,
  /// Current generation
  pub generation: u64,
}

#[cfg(test)]
mod tests {
  use std::collections::VecDeque;

  use super::*;
  use crate::core::network::replication::AckMessage;

  #[test]
  fn replication_manager_roles() {
    let server = ReplicationManager::new_server();
    assert!(server.is_server());
    assert!(!server.is_client());

    let client = ReplicationManager::new_client();
    assert!(client.is_client());
    assert!(!client.is_server());
  }

  #[test]
  fn spawn_networked_entity() {
    let mut manager = ReplicationManager::new_server();
    let mut world = World::new();

    let builder = NetworkEntityBuilder::new().server_owned();
    let entity = manager.spawn_networked(&mut world, builder);

    assert!(manager.is_networked(entity));
    assert!(manager.get_network_id(entity).is_some());

    let network_id = manager.get_network_id(entity).unwrap();
    assert_eq!(manager.get_entity(network_id), Some(entity));
  }

  #[test]
  fn despawn_networked_entity() {
    let mut manager = ReplicationManager::new_server();
    let mut world = World::new();

    let builder = NetworkEntityBuilder::new();
    let entity = manager.spawn_networked(&mut world, builder);
    let network_id = manager.get_network_id(entity).unwrap();

    let despawned_id = manager.despawn_networked(&mut world, entity);
    assert_eq!(despawned_id, Some(network_id));
    assert!(!manager.is_networked(entity));
    assert!(manager.get_entity(network_id).is_none());
  }

  #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
  struct Health(u32);

  impl Replicated for Health {
    fn component_id() -> ComponentTypeId {
      ComponentTypeId::from_name("Health")
    }

    fn component_name() -> &'static str {
      "Health"
    }
  }

  /// A server world with one networked entity carrying `Health(42)`.
  fn server_with_health() -> (ReplicationManager, World, NetworkId) {
    let mut server = ReplicationManager::new_server();
    server.register::<Health>();
    let mut world = World::new();
    let entity = server.spawn_networked(&mut world, NetworkEntityBuilder::new().server_owned());
    world.insert_one(entity, Health(42)).unwrap();
    let network_id = server.get_network_id(entity).unwrap();
    (server, world, network_id)
  }

  #[test]
  fn builder_components_land_on_the_spawned_entity() {
    let mut server = ReplicationManager::new_server();
    server.register::<Health>();
    let mut world = World::new();

    let entity = server.spawn_networked(
      &mut world,
      NetworkEntityBuilder::new().with_component(Health(7)),
    );

    assert_eq!(
      world.get::<Health>(entity).map(|h| (*h).clone()),
      Some(Health(7))
    );
    let network_id = server.get_network_id(entity).unwrap();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    assert_eq!(spawn.components.len(), 1);
  }

  #[test]
  fn full_state_sync_carries_component_state() {
    let (server, world, network_id) = server_with_health();

    let sync = server.build_full_state_sync(&world);

    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    client.handle_message(&mut client_world, ReplicationMessage::FullStateSync(sync));

    let entity = client.get_entity(network_id).unwrap();
    assert_eq!(
      client_world.get::<Health>(entity).map(|h| (*h).clone()),
      Some(Health(42))
    );
  }

  #[test]
  fn spawn_messages_carry_component_state() {
    let (server, world, network_id) = server_with_health();

    let spawn = server.spawn_message(&world, network_id).unwrap();

    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let entity = client
      .spawn_from_message(&mut client_world, &spawn)
      .unwrap();
    assert_eq!(
      client_world.get::<Health>(entity).map(|h| (*h).clone()),
      Some(Health(42))
    );
  }

  /// A server that records everything it sends.
  #[derive(Default)]
  struct RecordingServer {
    clients: Vec<ClientId>,
    sent: Vec<(ClientId, Channel, Vec<u8>)>,
    incoming: HashMap<(ClientId, Channel), VecDeque<Vec<u8>>>,
  }

  impl RecordingServer {
    fn deliver(&mut self, client_id: ClientId, channel: Channel, data: Vec<u8>) {
      self
        .incoming
        .entry((client_id, channel))
        .or_default()
        .push_back(data);
    }
  }

  impl ServerLike for RecordingServer {
    fn send(&mut self, client_id: ClientId, channel: Channel, data: &[u8]) {
      self.sent.push((client_id, channel, data.to_vec()));
    }

    fn broadcast(&mut self, channel: Channel, data: &[u8]) {
      for client_id in self.clients.clone() {
        self.send(client_id, channel, data);
      }
    }

    fn receive(&mut self, client_id: ClientId, channel: Channel) -> Option<Vec<u8>> {
      self
        .incoming
        .get_mut(&(client_id, channel))
        .and_then(VecDeque::pop_front)
    }

    fn clients(&self) -> Box<dyn Iterator<Item = ClientId> + '_> {
      Box::new(self.clients.iter().copied())
    }

    fn is_host(&self, _client_id: ClientId) -> bool {
      false
    }
  }

  /// Component batches the server sent on the unreliable replication channel.
  fn sent_batches(net: &RecordingServer) -> Vec<ComponentBatchMessage> {
    net
      .sent
      .iter()
      .filter(|(_, channel, _)| *channel == Channel::ReplicationUnreliable)
      .filter_map(
        |(_, _, bytes)| match ReplicationMessage::from_bytes(bytes) {
          Ok(ReplicationMessage::ComponentBatch(batch)) => Some(batch),
          _ => None,
        },
      )
      .collect()
  }

  /// Full-refresh batches the server sent on the unreliable replication channel.
  fn sent_refreshes(net: &RecordingServer) -> Vec<ComponentBatchMessage> {
    net
      .sent
      .iter()
      .filter(|(_, channel, _)| *channel == Channel::ReplicationUnreliable)
      .filter_map(
        |(_, _, bytes)| match ReplicationMessage::from_bytes(bytes) {
          Ok(ReplicationMessage::ComponentRefresh(batch)) => Some(batch),
          _ => None,
        },
      )
      .collect()
  }

  fn health_in(batches: Vec<ComponentBatchMessage>) -> Vec<Health> {
    batches
      .into_iter()
      .flat_map(|batch| batch.updates)
      .filter_map(|update| update.component.into_component::<Health>().ok())
      .collect()
  }

  /// Health values in the change batches the server sent.
  fn sent_health(net: &RecordingServer) -> Vec<Health> {
    health_in(sent_batches(net))
  }

  /// A client that records what it sends and replays queued messages.
  #[derive(Default)]
  struct FakeClient {
    incoming: HashMap<Channel, VecDeque<Vec<u8>>>,
    sent: Vec<(Channel, Vec<u8>)>,
  }

  impl ClientLike for FakeClient {
    fn send(&mut self, channel: Channel, data: &[u8]) {
      self.sent.push((channel, data.to_vec()));
    }

    fn receive(&mut self, channel: Channel) -> Option<Vec<u8>> {
      self
        .incoming
        .get_mut(&channel)
        .and_then(VecDeque::pop_front)
    }
  }

  impl FakeClient {
    fn deliver(&mut self, channel: Channel, msg: &ReplicationMessage) {
      self
        .incoming
        .entry(channel)
        .or_default()
        .push_back(msg.to_bytes().unwrap());
    }

    fn acks(&self) -> Vec<u64> {
      self
        .sent
        .iter()
        .filter(|(channel, _)| *channel == Channel::ReplicationUnreliable)
        .filter_map(|(_, bytes)| match ReplicationMessage::from_bytes(bytes) {
          Ok(ReplicationMessage::Ack(ack)) => Some(ack.generation),
          _ => None,
        })
        .collect()
    }
  }

  /// A batch carrying one `Health` value for `network_id`.
  fn health_batch(network_id: NetworkId, health: u32, generation: u64) -> ComponentBatchMessage {
    ComponentBatchMessage {
      updates: vec![ComponentUpdateMessage::new(
        network_id,
        SerializedComponent::from_component(&Health(health)).unwrap(),
      )],
      generation,
    }
  }

  #[test]
  fn periodic_refresh_resends_unchanged_components() {
    let (mut server, world, network_id) = server_with_health();
    let mut net = RecordingServer {
      clients: vec![1],
      ..Default::default()
    };
    server.register_client(1);
    // A change whose delta is "lost": never marked dirty, so no delta carries it
    let entity = server.get_entity(network_id).unwrap();
    *world.get_mut::<Health>(entity).unwrap() = Health(5);
    server.set_refresh_interval(60);

    for _ in 0..120 {
      server.broadcast_updates(&world, &mut net);
    }

    assert_eq!(health_in(sent_refreshes(&net)), vec![Health(5)]);
  }

  #[test]
  fn refresh_can_be_disabled() {
    let (mut server, world, _) = server_with_health();
    server.set_refresh_interval(0);
    let mut net = RecordingServer {
      clients: vec![1],
      ..Default::default()
    };
    server.register_client(1);

    for _ in 0..120 {
      server.broadcast_updates(&world, &mut net);
    }

    assert!(health_in(sent_refreshes(&net)).is_empty());
  }

  #[test]
  fn refresh_is_split_into_bounded_batches() {
    let mut server = ReplicationManager::new_server();
    server.register::<Health>();
    server.set_refresh_interval(1);
    let mut world = World::new();
    for i in 0..300 {
      let entity = server.spawn_networked(&mut world, NetworkEntityBuilder::new());
      world.insert_one(entity, Health(i)).unwrap();
    }
    let mut net = RecordingServer {
      clients: vec![1],
      ..Default::default()
    };
    server.register_client(1);

    // Generation 0 sends spawns; generation 1 is the first refresh
    server.broadcast_updates(&world, &mut net);
    server.broadcast_updates(&world, &mut net);

    let batches = sent_refreshes(&net);
    assert!(
      batches
        .iter()
        .all(|b| b.updates.len() <= REFRESH_BATCH_SIZE)
    );
    assert_eq!(batches.iter().map(|b| b.updates.len()).sum::<usize>(), 300);
  }

  #[test]
  fn unacknowledged_changes_are_resent_each_broadcast() {
    let (mut server, world, network_id) = server_with_health();
    let entity = server.get_entity(network_id).unwrap();
    let mut net = RecordingServer {
      clients: vec![1],
      ..Default::default()
    };
    server.register_client(1);
    server.mark_dirty::<Health>(entity);

    for _ in 0..3 {
      server.broadcast_updates(&world, &mut net);
    }

    assert_eq!(sent_health(&net), vec![Health(42); 3]);
  }

  #[test]
  fn an_ack_stops_the_resend() {
    let (mut server, world, network_id) = server_with_health();
    let entity = server.get_entity(network_id).unwrap();
    let mut net = RecordingServer {
      clients: vec![1],
      ..Default::default()
    };
    server.register_client(1);
    server.mark_dirty::<Health>(entity);
    server.broadcast_updates(&world, &mut net); // generation 0
    let ack = ReplicationMessage::Ack(AckMessage { generation: 0 })
      .to_bytes()
      .unwrap();
    net.deliver(1, Channel::ReplicationUnreliable, ack);

    server.broadcast_updates(&world, &mut net);

    assert_eq!(sent_health(&net).len(), 1);
    assert_eq!(server.client_acked_generation(1), Some(0));
  }

  #[test]
  fn client_applies_the_newest_batch_and_acknowledges_it() {
    let (server, world, network_id) = server_with_health();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(
      Channel::ReplicationReliable,
      &ReplicationMessage::SpawnEntity(spawn),
    );
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 50, 5)),
    );
    // A late, older batch must not roll the value back
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentBatch(health_batch(network_id, 30, 3)),
    );

    client.process_incoming(&mut client_world, &mut net);

    let entity = client.get_entity(network_id).unwrap();
    assert_eq!(
      client_world.get::<Health>(entity).map(|h| (*h).clone()),
      Some(Health(50))
    );
    assert_eq!(net.acks(), vec![5]);
  }

  #[test]
  fn refreshes_are_applied_but_not_acknowledged() {
    let (server, world, network_id) = server_with_health();
    let spawn = server.spawn_message(&world, network_id).unwrap();
    let mut client = ReplicationManager::new_client();
    client.register::<Health>();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net.deliver(
      Channel::ReplicationReliable,
      &ReplicationMessage::SpawnEntity(spawn),
    );
    net.deliver(
      Channel::ReplicationUnreliable,
      &ReplicationMessage::ComponentRefresh(health_batch(network_id, 70, 7)),
    );

    client.process_incoming(&mut client_world, &mut net);

    let entity = client.get_entity(network_id).unwrap();
    assert_eq!(
      client_world.get::<Health>(entity).map(|h| (*h).clone()),
      Some(Health(70))
    );
    assert!(net.acks().is_empty());
  }

  #[test]
  fn replication_leaves_game_messages_alone() {
    // Client side
    let mut client = ReplicationManager::new_client();
    let mut client_world = World::new();
    let mut net = FakeClient::default();
    net
      .incoming
      .entry(Channel::Unreliable)
      .or_default()
      .push_back(b"game".to_vec());
    net
      .incoming
      .entry(Channel::ReliableOrdered)
      .or_default()
      .push_back(b"chat".to_vec());
    client.process_incoming(&mut client_world, &mut net);
    assert_eq!(
      ClientLike::receive(&mut net, Channel::Unreliable),
      Some(b"game".to_vec())
    );
    assert_eq!(
      ClientLike::receive(&mut net, Channel::ReliableOrdered),
      Some(b"chat".to_vec())
    );

    // Server side
    let (mut server, world, _) = server_with_health();
    let mut recording = RecordingServer {
      clients: vec![1],
      ..Default::default()
    };
    server.register_client(1);
    recording.deliver(1, Channel::Unreliable, b"input".to_vec());
    server.broadcast_updates(&world, &mut recording);
    assert_eq!(
      recording.receive(1, Channel::Unreliable),
      Some(b"input".to_vec())
    );
  }

  #[test]
  fn replication_stats() {
    let manager = ReplicationManager::new_server();
    let stats = manager.stats();

    assert_eq!(stats.networked_entities, 0);
    assert_eq!(stats.generation, 0);
  }
}
