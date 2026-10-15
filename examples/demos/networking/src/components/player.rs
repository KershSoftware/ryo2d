//! Player component and related types

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

/// Player marker component
#[derive(Component, Debug, Clone, Copy)]
pub struct Player {
  /// Which player slot (0 = host/left, 1 = client/right)
  pub slot: u8,
  /// Is this player on the ground (or on the other player's head)?
  pub grounded: bool,
  /// The last horizontal input, -1 to 1: the host runs the player at it
  /// every step, whether or not new input arrived
  pub move_x: f32,
  /// Jump pressed since the host's last step
  pub jump: bool,
}

impl Player {
  pub fn new(slot: u8) -> Self {
    Self {
      slot,
      grounded: false,
      move_x: 0.0,
      jump: false,
    }
  }
}

/// Replicated player position for network sync
#[derive(Component, Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NetPlayerPosition {
  pub x: f32,
  pub y: f32,
  pub slot: u8,
}

impl Replicated for NetPlayerPosition {
  fn component_id() -> ComponentTypeId {
    ComponentTypeId::from_name("NetPlayerPosition")
  }

  fn component_name() -> &'static str {
    "NetPlayerPosition"
  }

  fn replication_priority() -> u8 {
    10 // High priority for player positions
  }
}

/// Player input RPC sent from client to server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerInputRpc {
  pub move_x: f32,
  pub jump: bool,
}

impl Rpc for PlayerInputRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("PlayerInputRpc")
  }

  fn rpc_name() -> &'static str {
    "PlayerInputRpc"
  }
}
