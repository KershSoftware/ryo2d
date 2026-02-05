//! Player component and related types

use serde::{Deserialize, Serialize};

use crate::constants::*;

/// Which direction the player is facing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Facing {
  #[default]
  Right,
  Left,
}

impl Facing {
  /// Get the sign for this facing direction (-1.0 for left, 1.0 for right)
  pub fn sign(&self) -> f32 {
    match self {
      Facing::Left => -1.0,
      Facing::Right => 1.0,
    }
  }
}

/// The current state of the player's leap mechanic
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LeapState {
  /// Standing still, ready to charge
  #[default]
  Idle,
  /// Charging a leap - holding space
  Charging {
    charge_time: f32,
    aim_angle: f32, // degrees from horizontal (0-90)
  },
  /// In the air after leaping
  Airborne,
  /// Brief recovery after landing
  Landing { timer: f32 },
}

impl LeapState {
  /// Check if player can start a new leap
  pub fn can_leap(&self) -> bool {
    matches!(self, LeapState::Idle)
  }

  /// Check if player is currently charging
  pub fn is_charging(&self) -> bool {
    matches!(self, LeapState::Charging { .. })
  }

  /// Check if player is airborne
  pub fn is_airborne(&self) -> bool {
    matches!(self, LeapState::Airborne)
  }

  /// Get charge time if charging, otherwise 0
  pub fn charge_time(&self) -> f32 {
    match self {
      LeapState::Charging { charge_time, .. } => *charge_time,
      _ => 0.0,
    }
  }

  /// Get aim angle if charging, otherwise default
  pub fn aim_angle(&self) -> f32 {
    match self {
      LeapState::Charging { aim_angle, .. } => *aim_angle,
      _ => DEFAULT_AIM_ANGLE,
    }
  }
}

/// Player marker component
#[derive(Debug, Clone, Copy)]
pub struct Player {
  /// Which player slot (0 = host/left, 1 = client/right)
  pub slot: u8,
  /// Is this player on the ground?
  pub grounded: bool,
  /// Current leap state
  pub leap_state: LeapState,
  /// Which direction the player is facing
  pub facing: Facing,
}

impl Player {
  pub fn new(slot: u8) -> Self {
    Self {
      slot,
      grounded: true,
      leap_state: LeapState::Idle,
      facing: if slot == 0 {
        Facing::Right
      } else {
        Facing::Left
      },
    }
  }

  /// Calculate leap force based on charge time
  pub fn calculate_leap_force(&self) -> f32 {
    let charge_time = self.leap_state.charge_time();
    let t = (charge_time / MAX_CHARGE_TIME).min(1.0);
    MIN_LEAP_FORCE + (MAX_LEAP_FORCE - MIN_LEAP_FORCE) * t
  }

  /// Check if currently overcharging
  pub fn is_overcharging(&self) -> bool {
    self.leap_state.charge_time() > OVERCHARGE_THRESHOLD
  }
}

/// Platform marker component
#[derive(Debug, Clone, Copy)]
pub struct Platform {
  pub width: f32,
  pub chunk_y: i32, // Which chunk this platform belongs to
  pub is_spawn: bool,
}

impl Platform {
  pub fn new(width: f32, chunk_y: i32) -> Self {
    Self {
      width,
      chunk_y,
      is_spawn: false,
    }
  }

  pub fn spawn_platform() -> Self {
    Self {
      width: SPAWN_PLATFORM_WIDTH,
      chunk_y: 0,
      is_spawn: true,
    }
  }
}

/// Network-serializable leap input
///
/// Uses the NetworkInput derive macro to enforce required fields at compile time.
/// The `facing_right` field is marked required because forgetting to set it
/// causes the player to face the wrong direction on the server.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ryo2d::prelude::NetworkInput)]
pub struct LeapInput {
  /// Whether the player is currently charging
  #[network_input(required)]
  pub charging: bool,
  /// Which direction the player is facing (REQUIRED - easy to forget!)
  #[network_input(required)]
  pub facing_right: bool,
  /// Current aim angle in degrees
  pub aim_angle: f32,
  /// Whether the player just released to leap
  pub release: bool,
  /// Time spent charging (for force calculation)
  pub charge_time: f32,
  /// Change in aim this frame
  pub aim_delta: f32,
}

/// Marker component for a light that follows a player
#[derive(Debug, Clone, Copy)]
pub struct PlayerLight {
  pub player_entity: Entity,
}

/// Component for platform lamps with oscillation data
#[derive(Debug, Clone, Copy)]
pub struct PlatformLamp {
  pub base_x: f32,
  pub base_y: f32,
  pub phase: f32,          // Random phase offset for variety
  pub platform_width: f32, // Width of platform for horizontal travel
  pub speed_mult: f32,     // Individual speed multiplier (0.5 - 1.5)
}

impl PlatformLamp {
  pub fn new(x: f32, y: f32, phase: f32, platform_width: f32, speed_mult: f32) -> Self {
    Self {
      base_x: x,
      base_y: y,
      phase,
      platform_width,
      speed_mult,
    }
  }
}

// Re-export Entity for use in components
use ryo2d::prelude::Entity;
