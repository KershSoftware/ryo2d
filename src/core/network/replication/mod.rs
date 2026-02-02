//! Network replication system for Ryo2D
//!
//! This module provides automatic synchronization of game state between
//! server and clients using a host-authoritative model.
//!
//! # Architecture
//!
//! The replication system consists of:
//! - **NetworkEntity** - Component marking entities for network sync
//! - **Replicated** - Trait for components that can be synchronized
//! - **ReplicationManager** - Handles spawning, despawning, and component updates
//! - **RPC System** - Remote procedure calls for game events
//!
//! # Quick Start
//!
//! ## Server Side
//!
//! ```ignore
//! // In your game's update loop:
//! let mut replication = ReplicationManager::new_server();
//! replication.register::<Transform>();
//! replication.register::<Velocity>();
//!
//! // Spawn a networked entity
//! let entity = replication.spawn_networked(
//!     &mut world,
//!     NetworkEntityBuilder::new()
//!         .with_component(Transform::new(100.0, 100.0))
//!         .with_component(Velocity::new(5.0, 0.0))
//! );
//!
//! // Broadcast updates to clients
//! replication.broadcast_updates(&mut world, &mut server);
//! ```
//!
//! ## Client Side
//!
//! ```ignore
//! // In your game's update loop:
//! let mut replication = ReplicationManager::new_client();
//! replication.register::<Transform>();
//! replication.register::<Velocity>();
//!
//! // Process incoming replication
//! replication.process_incoming(&mut world, &mut client);
//! ```

mod builder;
mod component;
mod dirty;
mod manager;
mod messages;
mod types;

// Re-export public types
pub use builder::NetworkEntityBuilder;
pub use component::{ComponentRegistry, ComponentTypeId, Replicated, SerializedComponent};
pub use dirty::DirtyTracker;
pub use manager::ReplicationManager;
pub use messages::{
  ComponentBatchMessage, ComponentUpdateMessage, DespawnEntityMessage, FullStateSyncMessage,
  ReplicationMessage, SpawnEntityMessage,
};
pub use types::{Authority, NetworkEntity, NetworkId};
