//! Builder for creating networked entities
//!
//! Provides a fluent API for spawning entities with network components.

use renet::ClientId;

use hecs::Entity;

use super::component::{Replicated, SerializedComponent};
use super::types::{Authority, NetworkEntity, NetworkId};
use crate::core::ecs::World;

/// Inserts one builder component into the spawned entity.
type ComponentInserter = Box<dyn FnOnce(&mut World, Entity) + Send + Sync>;

/// Builder for creating networked entities.
///
/// Provides a fluent API for configuring and spawning networked entities
/// with their initial components.
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
///
/// let client_id = 1;
/// let builder = NetworkEntityBuilder::new()
///     .with_owner(client_id)
///     .with_authority(Authority::Client(client_id))
///     .with_component(Transform::new(100.0, 200.0))
///     .with_component(Velocity::new(5.0, 0.0));
/// ```
pub struct NetworkEntityBuilder {
  /// The network ID (generated in `new`, replaced by `with_network_id`)
  network_id: NetworkId,

  /// Entity owner
  owner: ClientId,

  /// Entity authority
  authority: Authority,

  /// Serialized components to attach
  components: Vec<SerializedComponent>,

  /// Inserts the components given to `with_component` when the entity spawns
  inserters: Vec<ComponentInserter>,
}

impl NetworkEntityBuilder {
  /// Create a new builder with default settings.
  ///
  /// By default:
  /// - Network ID is auto-generated
  /// - Owner is server (0)
  /// - Authority is server
  pub fn new() -> Self {
    Self {
      network_id: NetworkId::new(),
      owner: 0,
      authority: Authority::Server,
      components: Vec::new(),
      inserters: Vec::new(),
    }
  }

  /// Set a specific network ID.
  ///
  /// Normally you should let the system generate IDs.
  /// Only use this when reconstructing entities from network messages.
  pub fn with_network_id(mut self, id: NetworkId) -> Self {
    self.network_id = id;
    self
  }

  /// Set the owner client ID.
  pub fn with_owner(mut self, owner: ClientId) -> Self {
    self.owner = owner;
    self
  }

  /// Set the authority.
  pub fn with_authority(mut self, authority: Authority) -> Self {
    self.authority = authority;
    self
  }

  /// Configure as a server-owned, server-authoritative entity.
  pub fn server_owned(mut self) -> Self {
    self.owner = 0;
    self.authority = Authority::Server;
    self
  }

  /// Configure as a client-owned entity with client authority.
  pub fn client_owned(mut self, client_id: ClientId) -> Self {
    self.owner = client_id;
    self.authority = Authority::Client(client_id);
    self
  }

  /// Add a replicated component.
  ///
  /// `ReplicationManager::spawn_networked` inserts it into the spawned entity,
  /// and the entity's spawn message carries its value to clients.
  pub fn with_component<T: Replicated + 'static>(mut self, component: T) -> Self {
    // Serialize for network transmission
    if let Ok(serialized) = SerializedComponent::from_component(&component) {
      self.components.push(serialized);
    }

    // Insert into the entity when it spawns
    self
      .inserters
      .push(Box::new(move |world: &mut World, entity| {
        world.add_component(entity, component);
      }));

    self
  }

  /// Add a pre-serialized component.
  ///
  /// Used when receiving spawn messages from the network.
  pub fn with_serialized_component(mut self, component: SerializedComponent) -> Self {
    self.components.push(component);
    self
  }

  /// Get the network ID the entity will spawn with.
  pub fn network_id(&self) -> NetworkId {
    self.network_id
  }

  /// Get the owner.
  pub fn owner(&self) -> ClientId {
    self.owner
  }

  /// Get the authority.
  pub fn authority(&self) -> Authority {
    self.authority
  }

  /// Get the serialized components.
  pub fn serialized_components(&self) -> &[SerializedComponent] {
    &self.components
  }

  /// Take ownership of the serialized components.
  pub fn take_serialized_components(self) -> Vec<SerializedComponent> {
    self.components
  }

  /// Build the NetworkEntity component.
  pub fn build_network_entity(&self) -> NetworkEntity {
    NetworkEntity {
      network_id: self.network_id(),
      owner: self.owner,
      authority: self.authority,
    }
  }

  /// Consume the builder, returning the network entity and the serialized
  /// form of every component given to it.
  ///
  /// To spawn the entity with its components, pass the builder to
  /// `ReplicationManager::spawn_networked` instead.
  pub fn build(self) -> (NetworkEntity, Vec<SerializedComponent>) {
    (self.build_network_entity(), self.components)
  }

  /// Insert the components given to [`with_component`](Self::with_component)
  /// into `entity` (called by `ReplicationManager::spawn_networked`).
  pub(crate) fn insert_components(self, world: &mut World, entity: Entity) {
    for insert in self.inserters {
      insert(world, entity);
    }
  }
}

impl std::fmt::Debug for NetworkEntityBuilder {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("NetworkEntityBuilder")
      .field("network_id", &self.network_id)
      .field("owner", &self.owner)
      .field("authority", &self.authority)
      .field("components", &self.components.len())
      .finish()
  }
}

impl Default for NetworkEntityBuilder {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde::{Deserialize, Serialize};

  #[derive(Debug, Clone, Serialize, Deserialize)]
  struct TestComponent {
    value: i32,
  }

  use super::super::component::ComponentTypeId;

  impl Replicated for TestComponent {
    fn component_id() -> ComponentTypeId {
      ComponentTypeId::from_name("TestComponent")
    }

    fn component_name() -> &'static str {
      "TestComponent"
    }
  }

  #[test]
  fn builder_defaults() {
    let builder = NetworkEntityBuilder::new();
    assert_eq!(builder.owner(), 0);
    assert_eq!(builder.authority(), Authority::Server);
  }

  #[test]
  fn builder_network_id_is_stable_and_matches_the_spawned_entity() {
    let builder = NetworkEntityBuilder::new();
    let id = builder.network_id();
    assert_eq!(builder.network_id(), id);

    let mut manager = super::super::manager::ReplicationManager::new_server();
    let mut world = World::new();
    let entity = manager.spawn_networked(&mut world, builder);
    assert_eq!(manager.get_network_id(entity), Some(id));
  }

  #[test]
  fn builder_server_owned() {
    let builder = NetworkEntityBuilder::new().server_owned();
    assert_eq!(builder.owner(), 0);
    assert_eq!(builder.authority(), Authority::Server);
  }

  #[test]
  fn builder_client_owned() {
    let builder = NetworkEntityBuilder::new().client_owned(5);
    assert_eq!(builder.owner(), 5);
    assert_eq!(builder.authority(), Authority::Client(5));
  }

  #[test]
  fn builder_with_component() {
    let builder = NetworkEntityBuilder::new().with_component(TestComponent { value: 42 });

    assert_eq!(builder.serialized_components().len(), 1);
  }

  #[test]
  fn builder_build() {
    let builder = NetworkEntityBuilder::new()
      .client_owned(3)
      .with_component(TestComponent { value: 100 });

    let (network_entity, components) = builder.build();

    assert_eq!(network_entity.owner, 3);
    assert_eq!(network_entity.authority, Authority::Client(3));
    assert_eq!(components.len(), 1);
  }
}
