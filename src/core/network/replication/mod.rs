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
//! use ryo2d::prelude::*;
//!
//! // Create replication manager
//! let mut replication = ReplicationManager::new_server();
//! let mut world = World::new();
//!
//! // Register replicated components
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
//! // In game loop: broadcast updates to clients
//! // replication.broadcast_updates(&mut world, &mut server);
//! ```
//!
//! ## Client Side
//!
//! ```ignore
//! use ryo2d::prelude::*;
//!
//! let mut replication = ReplicationManager::new_client();
//!
//! // Register same components as server
//! replication.register::<Transform>();
//! replication.register::<Velocity>();
//!
//! // In game loop: process incoming replication
//! // replication.process_incoming(&mut world, &mut client);
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
