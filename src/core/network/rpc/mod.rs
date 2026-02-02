//! Remote Procedure Call (RPC) system for Ryo2D
//!
//! This module provides a type-safe RPC system for sending game events
//! between server and clients that don't fit the component replication model.
//!
//! # When to Use RPCs
//!
//! Use RPCs for:
//! - Player input (jump, attack, use item)
//! - Game events (player scored, game started, chat message)
//! - One-shot actions that don't need continuous replication
//!
//! Use component replication for:
//! - Continuous state (position, health, score)
//! - State that needs interpolation
//! - Data that changes frequently
//!
//! # Quick Start
//!
//! ## Defining an RPC
//!
//! ```
//! use ryo2d::prelude::*;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! struct PlayerInputRpc {
//!     x: f32,
//!     y: f32,
//!     jump: bool,
//! }
//!
//! impl Rpc for PlayerInputRpc {
//!     fn rpc_id() -> RpcTypeId {
//!         RpcTypeId::from_name("PlayerInputRpc")
//!     }
//!
//!     fn rpc_name() -> &'static str {
//!         "PlayerInputRpc"
//!     }
//! }
//! ```
//!
//! ## Registering Handlers (Server)
//!
//! ```ignore
//! // Using PlayerInputRpc defined above:
//! let mut rpc_registry = RpcRegistry::new();
//!
//! rpc_registry.register::<PlayerInputRpc>(|world, rpc| {
//!     let sender = rpc.sender;
//!     let input = &rpc.payload;
//!
//!     // Handle the input
//!     println!("Player {} input: x={}, jump={}", sender, input.x, input.jump);
//!     Ok(())
//! });
//! ```
//!
//! ## Sending RPCs (Client)
//!
//! ```ignore
//! // Using PlayerInputRpc defined above:
//! let rpc = PlayerInputRpc { x: 1.0, y: 0.0, jump: true };
//! let serialized = SerializedRpc::from_rpc(&rpc, client_id, RpcTarget::Server)?;
//! let bytes = serialized.to_bytes()?;
//! client.send(Channel::ReliableOrdered, &bytes);
//! ```

mod registry;
mod types;

pub use registry::RpcRegistry;
pub use types::{ReceivedRpc, Rpc, RpcTarget, RpcTypeId, SerializedRpc};
