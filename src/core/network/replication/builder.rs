//! Builder for creating networked entities
//!
//! Provides a fluent API for spawning entities with network components.

use renet::ClientId;

use super::component::{Replicated, SerializedComponent};
use super::types::{Authority, NetworkEntity, NetworkId};

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
#[derive(Debug)]
pub struct NetworkEntityBuilder {
  /// The network ID (auto-generated if not specified)
  network_id: Option<NetworkId>,

  /// Entity owner
  owner: ClientId,

  /// Entity authority
  authority: Authority,

  /// Serialized components to attach
  components: Vec<SerializedComponent>,

  /// Component data for spawning (type-erased)
  spawn_data: Vec<Box<dyn std::any::Any + Send + Sync>>,
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
      network_id: None,
      owner: 0,
      authority: Authority::Server,
      components: Vec::new(),
      spawn_data: Vec::new(),
    }
  }

  /// Set a specific network ID.
  ///
  /// Normally you should let the system generate IDs.
  /// Only use this when reconstructing entities from network messages.
  pub fn with_network_id(mut self, id: NetworkId) -> Self {
    self.network_id = Some(id);
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
  /// The component is serialized for network transmission and stored
  /// for local entity spawning.
  pub fn with_component<T: Replicated + 'static>(mut self, component: T) -> Self {
    // Serialize for network transmission
    if let Ok(serialized) = SerializedComponent::from_component(&component) {
      self.components.push(serialized);
    }

    // Store for local spawning
    self.spawn_data.push(Box::new(component));

    self
  }

  /// Add a pre-serialized component.
  ///
  /// Used when receiving spawn messages from the network.
  pub fn with_serialized_component(mut self, component: SerializedComponent) -> Self {
    self.components.push(component);
    self
  }

  /// Get the network ID, generating one if needed.
  pub fn network_id(&self) -> NetworkId {
    self.network_id.unwrap_or_default()
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

  /// Consume the builder and return all spawn data.
  ///
  /// Returns (NetworkEntity, spawn_data) where spawn_data contains
  /// the boxed component values.
  pub fn build(self) -> (NetworkEntity, Vec<Box<dyn std::any::Any + Send + Sync>>) {
    let network_id = self.network_id.unwrap_or_default();

    let network_entity = NetworkEntity {
      network_id,
      owner: self.owner,
      authority: self.authority,
    };

    (network_entity, self.spawn_data)
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

    let (network_entity, spawn_data) = builder.build();

    assert_eq!(network_entity.owner, 3);
    assert_eq!(network_entity.authority, Authority::Client(3));
    assert_eq!(spawn_data.len(), 1);
  }
}
