//! Rocket component - MLRS-style projectile that arcs up then locks onto player
//!
//! Rockets launch upward with an arc, briefly calibrate on the player's position,
//! then fly in that direction without further tracking.

use ryo2d::prelude::Vec2;

/// Rocket flight phases
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RocketPhase {
  /// Initial launch - arcing upward
  Launch,
  /// Calibrating - tracking player position
  Calibrating,
  /// Locked - flying toward locked position
  Locked,
}

#[derive(Debug, Clone)]
pub struct Rocket {
  /// Current flight phase
  pub phase: RocketPhase,
  /// Current velocity
  pub velocity: Vec2,
  /// Timer for current phase
  pub phase_timer: f32,
  /// Locked target position (set during calibration)
  pub locked_target: Vec2,
  /// Rocket speed
  pub speed: f32,
}

/// Duration of launch arc phase
pub const ROCKET_LAUNCH_DURATION: f32 = 0.4;
/// Duration of calibration/tracking phase
pub const ROCKET_CALIBRATE_DURATION: f32 = 0.3;
/// Base rocket speed
pub const ROCKET_SPEED: f32 = 350.0;
/// Initial upward velocity for arc
pub const ROCKET_LAUNCH_VELOCITY: f32 = -200.0;
/// Horizontal spread for launch
pub const ROCKET_LAUNCH_SPREAD: f32 = 80.0;

impl Rocket {
  /// Create a new rocket launching from position with slight horizontal offset
  pub fn new(offset_x: f32) -> Self {
    // Launch upward with slight horizontal spread
    let velocity = Vec2::new(offset_x * 0.5, ROCKET_LAUNCH_VELOCITY);

    Self {
      phase: RocketPhase::Launch,
      velocity,
      phase_timer: ROCKET_LAUNCH_DURATION,
      locked_target: Vec2::ZERO,
      speed: ROCKET_SPEED,
    }
  }

  /// Update rocket state, returns true if rocket should be removed
  pub fn update(&mut self, current_pos: Vec2, player_pos: Vec2, dt: f32) -> bool {
    self.phase_timer -= dt;

    match self.phase {
      RocketPhase::Launch => {
        // Arc upward, slow down
        self.velocity.y += 300.0 * dt; // Gravity effect on arc

        if self.phase_timer <= 0.0 {
          // Transition to calibrating
          self.phase = RocketPhase::Calibrating;
          self.phase_timer = ROCKET_CALIBRATE_DURATION;
        }
      }
      RocketPhase::Calibrating => {
        // Track toward player during calibration
        let to_player = player_pos - current_pos;
        let dist = to_player.length();

        if dist > 1.0 {
          let target_vel = (to_player / dist) * self.speed;
          // Smoothly rotate toward player
          self.velocity = self.velocity.lerp(target_vel, 5.0 * dt);
        }

        // Update locked target continuously during calibration
        self.locked_target = player_pos;

        if self.phase_timer <= 0.0 {
          // Lock on and fly straight
          self.phase = RocketPhase::Locked;
          let to_target = self.locked_target - current_pos;
          let dist = to_target.length();
          if dist > 1.0 {
            self.velocity = (to_target / dist) * self.speed;
          }
        }
      }
      RocketPhase::Locked => {
        // Fly straight toward locked position
        // Slight acceleration
        let speed = self.velocity.length();
        if speed > 0.1 {
          self.velocity = (self.velocity / speed) * (speed + 100.0 * dt).min(self.speed * 1.3);
        }
      }
    }

    false // Never self-removes, game handles cleanup
  }

  /// Get rotation angle for rendering (points in velocity direction)
  pub fn rotation(&self) -> f32 {
    self.velocity.y.atan2(self.velocity.x)
  }

  /// Check if rocket is in dangerous phase (can hit player)
  pub fn is_dangerous(&self) -> bool {
    // Dangerous once calibrating or locked
    matches!(self.phase, RocketPhase::Calibrating | RocketPhase::Locked)
  }
}
