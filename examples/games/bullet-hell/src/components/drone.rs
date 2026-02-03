//! Drone enemy component - flies diagonally and fires lasers

use rand::Rng;

/// Spawn duration for drone fade-in effect
pub const DRONE_SPAWN_DURATION: f32 = 2.5;

/// Drone states for behavior control
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DroneState {
  /// Moving to a new position
  Moving,
  /// Pausing before firing laser
  Aiming,
  /// Firing the laser
  Firing,
  /// Cooldown after firing
  Cooldown,
}

#[derive(Debug, Clone)]
pub struct Drone {
  /// Current behavior state
  pub state: DroneState,
  /// Movement velocity
  pub velocity_x: f32,
  pub velocity_y: f32,
  /// Target position for current move
  pub target_x: f32,
  pub target_y: f32,
  /// Timer for current state
  pub state_timer: f32,
  /// Current aim position (slowly pans towards player)
  pub aim_x: f32,
  pub aim_y: f32,
  /// Locked laser target (set when firing starts, doesn't move)
  pub laser_target_x: f32,
  pub laser_target_y: f32,
  /// Whether laser is currently active
  pub laser_active: bool,
  /// Spawn timer for fade-in effect (0 = fully spawned)
  pub spawn_timer: f32,
}

/// How fast the laser aim pans towards the player (lerp factor per second)
pub const DRONE_AIM_SPEED: f32 = 1.5;

impl Drone {
  pub fn new(start_x: f32, start_y: f32) -> Self {
    let mut rng = rand::thread_rng();
    Self {
      state: DroneState::Cooldown, // Start in cooldown so it spawns first
      velocity_x: 0.0,
      velocity_y: 0.0,
      target_x: start_x,
      target_y: start_y,
      state_timer: rng.gen_range(0.5..1.5), // Random initial delay after spawn
      aim_x: start_x,
      aim_y: start_y + 200.0, // Start aiming below drone
      laser_target_x: 0.0,
      laser_target_y: 0.0,
      laser_active: false,
      spawn_timer: DRONE_SPAWN_DURATION,
    }
  }

  /// Check if drone is still spawning (fading in)
  pub fn is_spawning(&self) -> bool {
    self.spawn_timer > 0.0
  }

  /// Update spawn timer, returns true if just finished spawning
  pub fn update_spawn(&mut self, dt: f32) -> bool {
    if self.spawn_timer > 0.0 {
      self.spawn_timer -= dt;
      if self.spawn_timer <= 0.0 {
        self.spawn_timer = 0.0;
        return true;
      }
    }
    false
  }

  /// Get spawn progress (0.0 = just started, 1.0 = fully spawned)
  pub fn spawn_progress(&self) -> f32 {
    1.0 - (self.spawn_timer / DRONE_SPAWN_DURATION).clamp(0.0, 1.0)
  }

  /// Pick a new random target position to move to
  pub fn pick_new_target(&mut self, current_x: f32, current_y: f32, screen_w: f32, screen_h: f32) {
    let mut rng = rand::thread_rng();

    // Move diagonally - pick a position that's different from current
    let margin = 80.0;
    let min_x = margin;
    let max_x = screen_w - margin;
    let min_y = margin;
    let max_y = screen_h - 200.0; // Stay above ground

    // Pick random target
    self.target_x = rng.gen_range(min_x..max_x);
    self.target_y = rng.gen_range(min_y..max_y);

    // Ensure we move at least a minimum distance
    let min_distance = 150.0;
    let dx = self.target_x - current_x;
    let dy = self.target_y - current_y;
    let dist = (dx * dx + dy * dy).sqrt();

    if dist < min_distance {
      // Extend the movement
      let scale = min_distance / dist.max(1.0);
      self.target_x = (current_x + dx * scale).clamp(min_x, max_x);
      self.target_y = (current_y + dy * scale).clamp(min_y, max_y);
    }
  }

  /// Calculate velocity to reach target
  pub fn calculate_velocity(&mut self, current_x: f32, current_y: f32, speed: f32) {
    let dx = self.target_x - current_x;
    let dy = self.target_y - current_y;
    let dist = (dx * dx + dy * dy).sqrt();

    if dist > 1.0 {
      self.velocity_x = (dx / dist) * speed;
      self.velocity_y = (dy / dist) * speed;
    } else {
      self.velocity_x = 0.0;
      self.velocity_y = 0.0;
    }
  }

  /// Check if drone has reached its target
  pub fn reached_target(&self, current_x: f32, current_y: f32) -> bool {
    let dx = self.target_x - current_x;
    let dy = self.target_y - current_y;
    let dist = (dx * dx + dy * dy).sqrt();
    dist < 10.0
  }
}
