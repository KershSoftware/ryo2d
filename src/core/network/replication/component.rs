//! Replicated component trait and component registry
//!
//! This module provides the infrastructure for component replication:
//! - [`Replicated`] - Trait for components that can be synchronized over network
//! - [`ComponentTypeId`] - Unique identifier for component types
//! - [`ComponentRegistry`] - Registry for serialization/deserialization

use serde::{Serialize, de::DeserializeOwned};
use std::any::TypeId;
use std::collections::HashMap;

use crate::core::ecs::Component;
use crate::core::error::ReplicationError;

/// Unique identifier for a component type.
///
/// This is derived from the type's name and used to identify
/// components in network messages without transmitting full type info.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComponentTypeId(pub u32);

impl ComponentTypeId {
  /// Create a component type ID from a type name.
  ///
  /// Uses FNV-1a hash for deterministic, fast hashing.
  pub fn from_name(name: &str) -> Self {
    // FNV-1a hash
    let mut hash: u32 = 2166136261;
    for byte in name.bytes() {
      hash ^= byte as u32;
      hash = hash.wrapping_mul(16777619);
    }
    ComponentTypeId(hash)
  }

  /// Create from a raw value (for deserialization).
  pub fn from_raw(id: u32) -> Self {
    ComponentTypeId(id)
  }

  /// Get the raw u32 value.
  pub fn raw(&self) -> u32 {
    self.0
  }
}

impl std::fmt::Display for ComponentTypeId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

use serde::Deserialize;

/// Trait for components that can be replicated over the network.
///
/// Implement this trait for any component that should be automatically
/// synchronized between server and clients. The trait requires:
/// - [`Component`] - Must be a valid ECS component
/// - [`Serialize`] + [`DeserializeOwned`] - For network transmission
/// - [`Clone`] - For creating copies during replication
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// struct Health {
///     current: f32,
///     max: f32,
/// }
///
/// impl Replicated for Health {
///     fn component_id() -> ComponentTypeId {
///         ComponentTypeId::from_name("Health")
///     }
///
///     fn component_name() -> &'static str {
///         "Health"
///     }
/// }
/// ```
pub trait Replicated: Component + Serialize + DeserializeOwned + Clone {
  /// Get the unique type ID for this component.
  ///
  /// This should be deterministic (same ID on server and clients).
  fn component_id() -> ComponentTypeId;

  /// Get the human-readable name of this component.
  fn component_name() -> &'static str;

  /// Get the replication priority (1-255).
  ///
  /// Higher priority components are updated more frequently.
  /// Default is 1 (lowest priority). Use higher values for
  /// time-critical components like Transform.
  fn replication_priority() -> u8 {
    1
  }
}

/// Serialized component data for network transmission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializedComponent {
  /// The component type identifier.
  pub type_id: ComponentTypeId,
  /// Serialized component data (JSON text; sent as a string).
  #[serde(with = "data_encoding")]
  pub data: Vec<u8>,
}

/// `SerializedComponent::data` on the wire: JSON text as a string, about a
/// quarter the size of a byte array; non-UTF-8 data falls back to bytes.
/// Both forms are accepted when reading.
mod data_encoding {
  use serde::de::{self, SeqAccess, Visitor};
  use serde::{Deserializer, Serializer};

  pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    match std::str::from_utf8(data) {
      Ok(text) => serializer.serialize_str(text),
      Err(_) => serializer.serialize_bytes(data),
    }
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
    struct DataVisitor;

    impl<'de> Visitor<'de> for DataVisitor {
      type Value = Vec<u8>;

      fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("component data as a string or byte array")
      }

      fn visit_str<E: de::Error>(self, text: &str) -> Result<Vec<u8>, E> {
        Ok(text.as_bytes().to_vec())
      }

      fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
        Ok(bytes.to_vec())
      }

      fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<u8>, A::Error> {
        let mut data = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(byte) = seq.next_element()? {
          data.push(byte);
        }
        Ok(data)
      }
    }

    deserializer.deserialize_any(DataVisitor)
  }
}

impl SerializedComponent {
  /// Serialize a replicated component.
  pub fn from_component<T: Replicated>(component: &T) -> Result<Self, ReplicationError> {
    let data = serde_json::to_vec(component).map_err(|e| {
      ReplicationError::SerializationError(format!(
        "Failed to serialize {}: {}",
        T::component_name(),
        e
      ))
    })?;

    Ok(SerializedComponent {
      type_id: T::component_id(),
      data,
    })
  }

  /// Deserialize into a specific component type.
  pub fn into_component<T: Replicated>(self) -> Result<T, ReplicationError> {
    if self.type_id != T::component_id() {
      return Err(ReplicationError::TypeMismatch {
        expected: T::component_name().to_string(),
        found: format!("ComponentTypeId({})", self.type_id.0),
      });
    }

    serde_json::from_slice(&self.data).map_err(|e| {
      ReplicationError::DeserializationError(format!(
        "Failed to deserialize {}: {}",
        T::component_name(),
        e
      ))
    })
  }
}

/// Type-erased component serializer.
type ComponentSerializer =
  Box<dyn Fn(&dyn std::any::Any) -> Option<SerializedComponent> + Send + Sync>;

/// Type-erased component deserializer that applies to an entity.
type ComponentDeserializer = Box<
  dyn Fn(
      &SerializedComponent,
      &mut crate::core::ecs::World,
      hecs::Entity,
    ) -> Result<(), ReplicationError>
    + Send
    + Sync,
>;

/// Type-erased entity component serializer that queries the World.
type EntityComponentSerializer =
  Box<dyn Fn(&crate::core::ecs::World, hecs::Entity) -> Option<SerializedComponent> + Send + Sync>;

/// Registry for replicated component types.
///
/// The registry maintains serializers and deserializers for all registered
/// component types, enabling type-erased network transmission.
///
/// # Example
///
/// ```
/// use ryo2d::prelude::*;
///
/// let mut registry = ComponentRegistry::new();
/// registry.register::<Transform>();
/// registry.register::<Velocity>();
/// ```
pub struct ComponentRegistry {
  /// Maps TypeId to ComponentTypeId for quick lookup.
  type_to_id: HashMap<TypeId, ComponentTypeId>,

  /// Maps ComponentTypeId to type name for debugging.
  id_to_name: HashMap<ComponentTypeId, &'static str>,

  /// Serializers indexed by TypeId.
  serializers: HashMap<TypeId, ComponentSerializer>,

  /// Deserializers indexed by ComponentTypeId.
  deserializers: HashMap<ComponentTypeId, ComponentDeserializer>,

  /// Entity serializers indexed by ComponentTypeId (query World and serialize).
  entity_serializers: HashMap<ComponentTypeId, EntityComponentSerializer>,

  /// Priority for each component type.
  priorities: HashMap<ComponentTypeId, u8>,
}

impl ComponentRegistry {
  /// Create a new empty registry.
  pub fn new() -> Self {
    Self {
      type_to_id: HashMap::new(),
      id_to_name: HashMap::new(),
      serializers: HashMap::new(),
      deserializers: HashMap::new(),
      entity_serializers: HashMap::new(),
      priorities: HashMap::new(),
    }
  }

  /// Register a replicated component type.
  ///
  /// After registration, the component can be serialized and deserialized
  /// for network transmission.
  pub fn register<T: Replicated + 'static>(&mut self) {
    let type_id = TypeId::of::<T>();
    let component_id = T::component_id();

    self.type_to_id.insert(type_id, component_id);
    self.id_to_name.insert(component_id, T::component_name());
    self
      .priorities
      .insert(component_id, T::replication_priority());

    // Create serializer
    self.serializers.insert(
      type_id,
      Box::new(|any| {
        let component = any.downcast_ref::<T>()?;
        SerializedComponent::from_component(component).ok()
      }),
    );

    // Create deserializer
    self.deserializers.insert(
      component_id,
      Box::new(|serialized, world, entity| {
        let component: T = serde_json::from_slice(&serialized.data).map_err(|e| {
          ReplicationError::DeserializationError(format!(
            "Failed to deserialize {}: {}",
            T::component_name(),
            e
          ))
        })?;

        // Add or replace the component on the entity
        if world.get::<T>(entity).is_some() {
          // Entity has this component, need to update it
          if let Some(mut comp) = world.get_mut::<T>(entity) {
            *comp = component;
          }
        } else {
          // Entity doesn't have this component, add it
          world.add_component(entity, component);
        }

        Ok(())
      }),
    );

    // Create entity serializer (queries World for component)
    self.entity_serializers.insert(
      component_id,
      Box::new(|world, entity| {
        let component = world.get::<T>(entity)?;
        SerializedComponent::from_component(&*component).ok()
      }),
    );
  }

  /// Check if a component type is registered.
  pub fn is_registered<T: Replicated + 'static>(&self) -> bool {
    self.type_to_id.contains_key(&TypeId::of::<T>())
  }

  /// Get the ComponentTypeId for a registered type.
  pub fn get_component_id<T: Replicated + 'static>(&self) -> Option<ComponentTypeId> {
    self.type_to_id.get(&TypeId::of::<T>()).copied()
  }

  /// Get the component name for a ComponentTypeId.
  pub fn get_name(&self, id: ComponentTypeId) -> Option<&'static str> {
    self.id_to_name.get(&id).copied()
  }

  /// Get the replication priority for a component type.
  pub fn get_priority(&self, id: ComponentTypeId) -> u8 {
    self.priorities.get(&id).copied().unwrap_or(1)
  }

  /// Serialize a component by its TypeId.
  pub fn serialize(
    &self,
    type_id: TypeId,
    component: &dyn std::any::Any,
  ) -> Option<SerializedComponent> {
    self.serializers.get(&type_id).and_then(|f| f(component))
  }

  /// Serialize a component from an entity by ComponentTypeId.
  ///
  /// This queries the World for the component and serializes it if present.
  pub fn serialize_from_entity(
    &self,
    component_id: ComponentTypeId,
    world: &crate::core::ecs::World,
    entity: hecs::Entity,
  ) -> Option<SerializedComponent> {
    self
      .entity_serializers
      .get(&component_id)
      .and_then(|f| f(world, entity))
  }

  /// Deserialize and apply a component to an entity.
  pub fn deserialize_and_apply(
    &self,
    serialized: &SerializedComponent,
    world: &mut crate::core::ecs::World,
    entity: hecs::Entity,
  ) -> Result<(), ReplicationError> {
    let deserializer = self
      .deserializers
      .get(&serialized.type_id)
      .ok_or(ReplicationError::UnknownComponent(serialized.type_id))?;

    deserializer(serialized, world, entity)
  }

  /// Get all registered component type IDs.
  pub fn registered_types(&self) -> impl Iterator<Item = ComponentTypeId> + '_ {
    self.id_to_name.keys().copied()
  }
}

impl Default for ComponentRegistry {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn component_data_travels_as_json_text() {
    let component = SerializedComponent {
      type_id: ComponentTypeId(7),
      data: br#"{"hp":42}"#.to_vec(),
    };

    let json = serde_json::to_string(&component).unwrap();

    assert_eq!(json, r#"{"type_id":7,"data":"{\"hp\":42}"}"#);
    let back: SerializedComponent = serde_json::from_str(&json).unwrap();
    assert_eq!(back.data, component.data);
  }

  #[test]
  fn component_data_still_reads_the_byte_array_form() {
    let back: SerializedComponent =
      serde_json::from_str(r#"{"type_id":7,"data":[123,125]}"#).unwrap();
    assert_eq!(back.data, b"{}".to_vec());
  }

  #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
  struct TestComponent {
    value: i32,
  }

  impl Replicated for TestComponent {
    fn component_id() -> ComponentTypeId {
      ComponentTypeId::from_name("TestComponent")
    }

    fn component_name() -> &'static str {
      "TestComponent"
    }

    fn replication_priority() -> u8 {
      5
    }
  }

  #[test]
  fn component_type_id_deterministic() {
    let id1 = ComponentTypeId::from_name("TestComponent");
    let id2 = ComponentTypeId::from_name("TestComponent");
    let id3 = ComponentTypeId::from_name("OtherComponent");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
  }

  #[test]
  fn serialized_component_roundtrip() {
    let original = TestComponent { value: 42 };
    let serialized = SerializedComponent::from_component(&original).unwrap();
    let deserialized: TestComponent = serialized.into_component().unwrap();

    assert_eq!(original, deserialized);
  }

  #[test]
  fn component_registry_register() {
    let mut registry = ComponentRegistry::new();
    assert!(!registry.is_registered::<TestComponent>());

    registry.register::<TestComponent>();
    assert!(registry.is_registered::<TestComponent>());

    let id = registry.get_component_id::<TestComponent>();
    assert!(id.is_some());
    assert_eq!(id.unwrap(), TestComponent::component_id());
  }

  #[test]
  fn component_registry_priority() {
    let mut registry = ComponentRegistry::new();
    registry.register::<TestComponent>();

    let id = TestComponent::component_id();
    assert_eq!(registry.get_priority(id), 5);
  }

  #[test]
  fn component_registry_name() {
    let mut registry = ComponentRegistry::new();
    registry.register::<TestComponent>();

    let id = TestComponent::component_id();
    assert_eq!(registry.get_name(id), Some("TestComponent"));
  }
}
