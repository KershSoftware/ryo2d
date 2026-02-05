//! Network message types for multiplayer

use ryo2d::core::network::action::{ActionContext, NetworkAction};
use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{LeapInput, LeapState, Player};

/// Client -> Server: Player leap input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeapInputRpc {
  pub input: LeapInput,
}

impl Rpc for LeapInputRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("LeapInputRpc")
  }

  fn rpc_name() -> &'static str {
    "LeapInputRpc"
  }
}

/// Server -> Client: Player position update (unreliable, high frequency)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdate {
  pub client_id: ClientId,
  pub slot: u8,
  pub x: f32,
  pub y: f32,
  pub vx: f32,
  pub vy: f32,
  pub leap_state: NetworkLeapState,
  pub facing_right: bool,
  pub best_height: f32,
}

/// Network-friendly leap state (simplified for serialization)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum NetworkLeapState {
  #[default]
  Idle,
  Charging {
    charge_time: f32,
    aim_angle: f32,
  },
  Airborne,
  Landing {
    timer: f32,
  },
}

impl From<crate::components::LeapState> for NetworkLeapState {
  fn from(state: crate::components::LeapState) -> Self {
    match state {
      crate::components::LeapState::Idle => NetworkLeapState::Idle,
      crate::components::LeapState::Charging {
        charge_time,
        aim_angle,
      } => NetworkLeapState::Charging {
        charge_time,
        aim_angle,
      },
      crate::components::LeapState::Airborne => NetworkLeapState::Airborne,
      crate::components::LeapState::Landing { timer } => NetworkLeapState::Landing { timer },
    }
  }
}

impl From<NetworkLeapState> for crate::components::LeapState {
  fn from(state: NetworkLeapState) -> Self {
    match state {
      NetworkLeapState::Idle => crate::components::LeapState::Idle,
      NetworkLeapState::Charging {
        charge_time,
        aim_angle,
      } => crate::components::LeapState::Charging {
        charge_time,
        aim_angle,
      },
      NetworkLeapState::Airborne => crate::components::LeapState::Airborne,
      NetworkLeapState::Landing { timer } => crate::components::LeapState::Landing { timer },
    }
  }
}

/// Server -> Client: Game seed for deterministic platform generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSeedRpc {
  pub seed: u64,
}

impl Rpc for GameSeedRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("GameSeedRpc")
  }

  fn rpc_name() -> &'static str {
    "GameSeedRpc"
  }
}

/// Client -> Server: Request to respawn (kept for backwards compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespawnRpc;

impl Rpc for RespawnRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("RespawnRpc")
  }

  fn rpc_name() -> &'static str {
    "RespawnRpc"
  }
}

/// Respawn action using NetworkAction trait for dual local+network execution
///
/// This action respawns a player at their spawn position. Using NetworkAction
/// ensures the respawn happens both locally (for immediate feedback) and is
/// sent to the server (for synchronization).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RespawnAction {
  /// Which player slot to respawn
  pub slot: u8,
  /// Spawn X position
  pub spawn_x: f32,
  /// Spawn Y position
  pub spawn_y: f32,
}

impl NetworkAction for RespawnAction {
  fn action_id() -> RpcTypeId {
    RpcTypeId::from_name("RespawnAction")
  }

  fn action_name() -> &'static str {
    "RespawnAction"
  }

  fn apply(&self, ctx: &mut ActionContext) {
    // Find the player entity by slot and reset its state
    let mut target_entity = None;
    for (entity, player) in ctx.world.query::<(Entity, &Player)>().iter() {
      if player.slot == self.slot {
        target_entity = Some(entity);
        break;
      }
    }

    let Some(entity) = target_entity else {
      return;
    };

    // Reset position
    if let Some(mut transform) = ctx.world.get_component_mut::<Transform>(entity) {
      transform.x = self.spawn_x;
      transform.y = self.spawn_y;
    }

    // Reset velocity
    if let Some(mut vel) = ctx.world.get_component_mut::<Velocity>(entity) {
      vel.dx = 0.0;
      vel.dy = 0.0;
    }

    // Reset player state
    if let Some(mut player) = ctx.world.get_component_mut::<Player>(entity) {
      player.grounded = true;
      player.leap_state = LeapState::Idle;
    }

    // Force prediction correction so client accepts server position
    if let Some(mut predicted) = ctx.world.get_component_mut::<Predicted>(entity) {
      predicted.force_correction();
    }
  }
}

/// Server -> Client: Game seed and session info for reconnection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInitRpc {
  pub seed: u64,
  pub session_id: u64,
  pub checkpoint_x: Option<f32>,
  pub checkpoint_y: Option<f32>,
  pub checkpoint_height: Option<f32>,
}

impl Rpc for GameInitRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("GameInitRpc")
  }

  fn rpc_name() -> &'static str {
    "GameInitRpc"
  }
}

/// Client -> Server: Session reconnect request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReconnectRpc {
  pub session_id: u64,
}

impl Rpc for SessionReconnectRpc {
  fn rpc_id() -> RpcTypeId {
    RpcTypeId::from_name("SessionReconnectRpc")
  }

  fn rpc_name() -> &'static str {
    "SessionReconnectRpc"
  }
}

/// Stored session data for reconnection
#[derive(Debug, Clone, Default)]
pub struct SessionData {
  pub checkpoint_x: f32,
  pub checkpoint_y: f32,
  pub checkpoint_height: f32,
  pub max_height: f32,
}

/// Helper to serialize a value to bytes
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, NetworkError> {
  serde_json::to_vec(value).map_err(|e| NetworkError::SendError(e.to_string()))
}

/// Helper to deserialize bytes to a value
pub fn deserialize<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, NetworkError> {
  serde_json::from_slice(bytes).map_err(|e| NetworkError::ReceiveError(e.to_string()))
}
