//! Network message types for multiplayer

use ryo2d::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::LeapInput;

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

/// Client -> Server: Request to respawn
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

/// Helper to serialize a value to bytes
pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, NetworkError> {
  serde_json::to_vec(value).map_err(|e| NetworkError::SendError(e.to_string()))
}

/// Helper to deserialize bytes to a value
pub fn deserialize<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, NetworkError> {
  serde_json::from_slice(bytes).map_err(|e| NetworkError::ReceiveError(e.to_string()))
}
