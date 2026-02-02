//! Network messages for replication
//!
//! Defines the message types used for entity and component synchronization.

use serde::{Deserialize, Serialize};

use super::component::SerializedComponent;
use super::types::{Authority, NetworkId};
use renet::ClientId;

/// All replication message types.
///
/// Messages are sent over appropriate channels based on their reliability needs:
/// - Spawns/Despawns → ReliableOrdered (must not be lost)
/// - Component updates → Unreliable (frequent, can miss some)
/// - RPCs → ReliableOrdered (game events)
/// - Full state sync → ReliableOrdered (new client init)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplicationMessage {
    /// Spawn a new networked entity
    SpawnEntity(SpawnEntityMessage),

    /// Despawn a networked entity
    DespawnEntity(DespawnEntityMessage),

    /// Update a single component
    ComponentUpdate(ComponentUpdateMessage),

    /// Batch of component updates (more efficient)
    ComponentBatch(ComponentBatchMessage),

    /// Full state sync for a new client
    FullStateSync(FullStateSyncMessage),

    /// Remote procedure call
    Rpc(RpcMessage),
}

impl ReplicationMessage {
    /// Serialize the message to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize a message from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Message to spawn a new networked entity.
///
/// Sent from server to clients when a new networked entity is created.
/// Includes the initial state of all replicated components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnEntityMessage {
    /// The network ID for this entity
    pub network_id: NetworkId,

    /// Who owns this entity
    pub owner: ClientId,

    /// Who has authority to modify it
    pub authority: Authority,

    /// Initial component data
    pub components: Vec<SerializedComponent>,
}

impl SpawnEntityMessage {
    /// Create a new spawn message.
    pub fn new(network_id: NetworkId, owner: ClientId, authority: Authority) -> Self {
        Self {
            network_id,
            owner,
            authority,
            components: Vec::new(),
        }
    }

    /// Add a serialized component to the spawn message.
    pub fn with_component(mut self, component: SerializedComponent) -> Self {
        self.components.push(component);
        self
    }

    /// Add multiple components.
    pub fn with_components(mut self, components: Vec<SerializedComponent>) -> Self {
        self.components.extend(components);
        self
    }
}

/// Message to despawn a networked entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DespawnEntityMessage {
    /// The network ID of the entity to despawn
    pub network_id: NetworkId,
}

impl DespawnEntityMessage {
    /// Create a new despawn message.
    pub fn new(network_id: NetworkId) -> Self {
        Self { network_id }
    }
}

/// Message to update a single component on a networked entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentUpdateMessage {
    /// The network ID of the entity
    pub network_id: NetworkId,

    /// The serialized component data
    pub component: SerializedComponent,

    /// Sequence number for ordering (optional, 0 if not used)
    pub sequence: u32,
}

impl ComponentUpdateMessage {
    /// Create a new component update message.
    pub fn new(network_id: NetworkId, component: SerializedComponent) -> Self {
        Self {
            network_id,
            component,
            sequence: 0,
        }
    }

    /// Set the sequence number.
    pub fn with_sequence(mut self, sequence: u32) -> Self {
        self.sequence = sequence;
        self
    }
}

/// Batch of component updates for efficiency.
///
/// Multiple component updates are combined into a single message
/// to reduce network overhead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentBatchMessage {
    /// All component updates in this batch
    pub updates: Vec<ComponentUpdateMessage>,

    /// Generation/frame number for this batch
    pub generation: u64,
}

impl ComponentBatchMessage {
    /// Create a new empty batch.
    pub fn new(generation: u64) -> Self {
        Self {
            updates: Vec::new(),
            generation,
        }
    }

    /// Add an update to the batch.
    pub fn add(&mut self, update: ComponentUpdateMessage) {
        self.updates.push(update);
    }

    /// Check if the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.updates.is_empty()
    }

    /// Get the number of updates in the batch.
    pub fn len(&self) -> usize {
        self.updates.len()
    }
}

/// Full state synchronization for new clients.
///
/// Sent when a client first connects to bring them up to date
/// with all current networked entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullStateSyncMessage {
    /// All entities and their current state
    pub entities: Vec<SpawnEntityMessage>,

    /// Current server generation
    pub generation: u64,
}

impl FullStateSyncMessage {
    /// Create a new full state sync message.
    pub fn new(generation: u64) -> Self {
        Self {
            entities: Vec::new(),
            generation,
        }
    }

    /// Add an entity to the sync.
    pub fn add_entity(&mut self, entity: SpawnEntityMessage) {
        self.entities.push(entity);
    }

    /// Check if the sync is empty.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// Remote procedure call message.
///
/// RPCs are used for game events that don't fit the component model,
/// such as "player jumped", "game started", "chat message", etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcMessage {
    /// Unique identifier for the RPC type
    pub rpc_id: u32,

    /// Human-readable RPC name (for debugging)
    pub name: String,

    /// Serialized RPC payload
    pub payload: Vec<u8>,

    /// Who sent this RPC (0 for server)
    pub sender: ClientId,

    /// Target of the RPC
    pub target: RpcTarget,
}

/// Target for an RPC message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RpcTarget {
    /// Send to server only (client → server)
    Server,

    /// Send to a specific client (server → client)
    Client(ClientId),

    /// Broadcast to all clients (server → all)
    Broadcast,

    /// Broadcast to all clients except one (server → all except sender)
    BroadcastExcept(ClientId),
}

impl RpcMessage {
    /// Create a new RPC message.
    pub fn new<T: Serialize>(
        rpc_id: u32,
        name: impl Into<String>,
        payload: &T,
        sender: ClientId,
        target: RpcTarget,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            rpc_id,
            name: name.into(),
            payload: serde_json::to_vec(payload)?,
            sender,
            target,
        })
    }

    /// Deserialize the payload.
    pub fn decode_payload<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::network::replication::ComponentTypeId;

    #[test]
    fn replication_message_roundtrip() {
        let msg = ReplicationMessage::DespawnEntity(DespawnEntityMessage::new(NetworkId::from_raw(42)));

        let bytes = msg.to_bytes().unwrap();
        let decoded = ReplicationMessage::from_bytes(&bytes).unwrap();

        match decoded {
            ReplicationMessage::DespawnEntity(despawn) => {
                assert_eq!(despawn.network_id.raw(), 42);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn spawn_entity_message() {
        let mut msg = SpawnEntityMessage::new(
            NetworkId::from_raw(1),
            0,
            Authority::Server,
        );

        let component = SerializedComponent {
            type_id: ComponentTypeId::from_name("Test"),
            data: vec![1, 2, 3],
        };

        msg = msg.with_component(component);
        assert_eq!(msg.components.len(), 1);
    }

    #[test]
    fn component_batch_message() {
        let mut batch = ComponentBatchMessage::new(1);
        assert!(batch.is_empty());

        let update = ComponentUpdateMessage::new(
            NetworkId::from_raw(1),
            SerializedComponent {
                type_id: ComponentTypeId::from_name("Test"),
                data: vec![],
            },
        );

        batch.add(update);
        assert_eq!(batch.len(), 1);
        assert!(!batch.is_empty());
    }

    #[test]
    fn rpc_message_roundtrip() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestPayload {
            value: i32,
        }

        let rpc = RpcMessage::new(
            1,
            "test_rpc",
            &TestPayload { value: 42 },
            0,
            RpcTarget::Broadcast,
        )
        .unwrap();

        let decoded: TestPayload = rpc.decode_payload().unwrap();
        assert_eq!(decoded.value, 42);
    }
}
