//! ECS components for the raycasting system.
//!
//! Provides camera, sprite, light, and door components that can be attached to
//! entities in the ECS [`World`](crate::core::ecs::World) to participate in the
//! GPU-accelerated raycasting renderer.

// ---------------------------------------------------------------------------
// RaycastCamera
// ---------------------------------------------------------------------------

/// Camera component for the raycasting renderer.
///
/// Defines the look direction and field of view. The camera plane is
/// always perpendicular to the direction vector; its length controls the FOV.
#[derive(Debug, Clone, PartialEq)]
pub struct RaycastCamera {
  /// Unit look vector.
  pub direction: (f32, f32),
  /// Camera plane (perpendicular to direction, length = tan(fov/2)).
  pub plane: (f32, f32),
  /// Vertical eye height in the cell (0.5 = eye level).
  pub height: f32,
}

impl RaycastCamera {
  /// Create a new camera looking along +X with the given field of view.
  ///
  /// `fov_degrees` is the horizontal field of view in degrees (typically 66).
  pub fn new(fov_degrees: f32) -> Self {
    let plane_len = (fov_degrees.to_radians() / 2.0).tan();
    Self {
      direction: (1.0, 0.0),
      plane: (0.0, plane_len),
      height: 0.5,
    }
  }

  /// Set the look angle in radians (0 = +X, PI/2 = +Y).
  pub fn set_angle(&mut self, angle: f32) {
    let cos = angle.cos();
    let sin = angle.sin();
    let plane_len = self.plane_length();
    self.direction = (cos, sin);
    // Plane is perpendicular: rotate direction by +90 degrees and scale.
    self.plane = (-sin * plane_len, cos * plane_len);
  }

  /// Get the current look angle in radians via `atan2`.
  pub fn angle(&self) -> f32 {
    self.direction.1.atan2(self.direction.0)
  }

  /// Rotate the camera by `delta` radians.
  pub fn rotate(&mut self, delta: f32) {
    let new_angle = self.angle() + delta;
    self.set_angle(new_angle);
  }

  /// Compute the length of the camera plane vector.
  fn plane_length(&self) -> f32 {
    (self.plane.0 * self.plane.0 + self.plane.1 * self.plane.1).sqrt()
  }
}

// ---------------------------------------------------------------------------
// RaycastSprite
// ---------------------------------------------------------------------------

/// Sprite component rendered inside the raycasting world.
///
/// Sprites are camera-facing billboards by default. When `directional` is
/// `true`, the renderer selects one of `num_angles` sub-frames based on the
/// viewing angle.
#[derive(Debug, Clone, PartialEq)]
pub struct RaycastSprite {
  /// Texture/sprite index used by the renderer.
  pub texture_id: u32,
  /// Scale relative to one grid cell (1.0, 1.0 = exactly one cell).
  pub scale: (f32, f32),
  /// If true, the sprite has multiple angle frames.
  pub directional: bool,
  /// Number of angle frames (only used when `directional` is true).
  pub num_angles: u8,
  /// Vertical offset from the floor (in world units).
  pub v_offset: f32,
}

impl RaycastSprite {
  /// Create a new sprite with sensible defaults.
  pub fn new(texture_id: u32) -> Self {
    Self {
      texture_id,
      scale: (1.0, 1.0),
      directional: false,
      num_angles: 1,
      v_offset: 0.0,
    }
  }
}

// ---------------------------------------------------------------------------
// RaycastLight
// ---------------------------------------------------------------------------

/// Point light component for the raycasting world.
///
/// Lights attenuate with distance and can optionally flicker for atmospheric
/// effects (torches, candles, etc.).
#[derive(Debug, Clone, PartialEq)]
pub struct RaycastLight {
  /// Maximum radius of the light in world units.
  pub radius: f32,
  /// Light colour as (r, g, b) in 0.0..1.0.
  pub color: (f32, f32, f32),
  /// Brightness multiplier.
  pub intensity: f32,
  /// Whether the light randomly flickers.
  pub flicker: bool,
  /// World-space height of the light (0.5 = mid-wall).
  pub height: f32,
  /// Falloff exponent (1.0 = linear, 2.0 = quadratic, 3.0 = cubic).
  pub falloff: f32,
}

impl RaycastLight {
  /// Create a new light source. `flicker` defaults to `false`.
  pub fn new(radius: f32, color: (f32, f32, f32), intensity: f32) -> Self {
    Self {
      radius,
      color,
      intensity,
      flicker: false,
      height: 0.5,
      falloff: 2.0,
    }
  }
}

// ---------------------------------------------------------------------------
// DoorSpeed / DoorState / RaycastDoor
// ---------------------------------------------------------------------------

/// How fast a door opens and closes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DoorSpeed {
  /// 0.5 cells per second.
  Slow,
  /// 1.0 cells per second.
  Normal,
  /// 2.0 cells per second.
  Fast,
  /// Custom speed in cells per second.
  Custom(f32),
}

impl DoorSpeed {
  /// Return the speed in cells per second.
  pub fn cells_per_second(&self) -> f32 {
    match self {
      DoorSpeed::Slow => 0.5,
      DoorSpeed::Normal => 1.0,
      DoorSpeed::Fast => 2.0,
      DoorSpeed::Custom(v) => {
        debug_assert!(*v > 0.0, "DoorSpeed::Custom must be positive, got {v}");
        v.max(0.001)
      }
    }
  }
}

/// The current animation state of a door.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DoorState {
  /// Fully closed.
  Closed,
  /// Currently sliding open.
  Opening,
  /// Fully open; `timer` counts up toward auto-close.
  Open {
    /// Seconds elapsed since the door finished opening.
    timer: f32,
  },
  /// Currently sliding closed.
  Closing,
}

/// Door component that ties a map cell to an animated door entity.
///
/// Use [`activate`](Self::activate) to begin opening the door and
/// [`tick`](Self::tick) each frame to advance the animation.
#[derive(Debug, Clone, PartialEq)]
pub struct RaycastDoor {
  /// Map column of the door cell.
  pub map_x: u32,
  /// Map row of the door cell.
  pub map_y: u32,
  /// Current animation state.
  pub state: DoorState,
  /// Speed at which the door opens/closes.
  pub speed: DoorSpeed,
  /// Current openness (0.0 = fully closed, 1.0 = fully open).
  pub open_amount: f32,
  /// How many seconds the door stays open before auto-closing.
  pub open_duration: f32,
}

impl RaycastDoor {
  /// Create a new closed door at the given map position.
  pub fn new(map_x: u32, map_y: u32, speed: DoorSpeed) -> Self {
    Self {
      map_x,
      map_y,
      state: DoorState::Closed,
      speed,
      open_amount: 0.0,
      open_duration: 3.0,
    }
  }

  /// Attempt to activate (open) the door.
  ///
  /// Only transitions from `Closed` to `Opening`; other states are a no-op.
  pub fn activate(&mut self) {
    if matches!(self.state, DoorState::Closed) {
      self.state = DoorState::Opening;
    }
  }

  /// Advance the door animation by `dt` seconds.
  ///
  /// Returns the current `open_amount` after the update.
  pub fn tick(&mut self, dt: f32) -> f32 {
    let speed = self.speed.cells_per_second();
    match self.state {
      DoorState::Closed => {}
      DoorState::Opening => {
        self.open_amount += speed * dt;
        if self.open_amount >= 1.0 {
          self.open_amount = 1.0;
          self.state = DoorState::Open { timer: 0.0 };
        }
      }
      DoorState::Open { timer } => {
        let new_timer = timer + dt;
        if new_timer >= self.open_duration {
          self.state = DoorState::Closing;
        } else {
          self.state = DoorState::Open { timer: new_timer };
        }
      }
      DoorState::Closing => {
        self.open_amount -= speed * dt;
        if self.open_amount <= 0.0 {
          self.open_amount = 0.0;
          self.state = DoorState::Closed;
        }
      }
    }
    self.open_amount
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;
  use std::f32::consts::PI;

  const EPSILON: f32 = 1e-5;

  #[test]
  fn camera_default_direction() {
    let cam = RaycastCamera::new(66.0);
    assert!((cam.direction.0 - 1.0).abs() < EPSILON);
    assert!((cam.direction.1 - 0.0).abs() < EPSILON);
  }

  #[test]
  fn camera_set_angle() {
    let mut cam = RaycastCamera::new(66.0);
    cam.set_angle(PI / 2.0);
    assert!((cam.direction.0 - 0.0).abs() < EPSILON);
    assert!((cam.direction.1 - 1.0).abs() < EPSILON);
  }

  #[test]
  fn camera_rotate() {
    let mut cam = RaycastCamera::new(66.0);
    cam.rotate(PI);
    assert!((cam.direction.0 - (-1.0)).abs() < EPSILON);
    assert!((cam.direction.1 - 0.0).abs() < EPSILON);
  }

  #[test]
  fn camera_plane_perpendicular() {
    let mut cam = RaycastCamera::new(66.0);
    for &angle in &[0.0, PI / 4.0, PI / 2.0, PI, -PI / 3.0] {
      cam.set_angle(angle);
      let dot = cam.direction.0 * cam.plane.0 + cam.direction.1 * cam.plane.1;
      assert!(
        dot.abs() < EPSILON,
        "plane not perpendicular at angle {angle}: dot = {dot}"
      );
    }
  }

  #[test]
  fn camera_fov_plane_length() {
    let cam = RaycastCamera::new(66.0);
    let expected = (66.0_f32.to_radians() / 2.0).tan();
    let len = cam.plane_length();
    assert!(
      (len - expected).abs() < EPSILON,
      "plane length {len} != expected {expected}"
    );
  }

  #[test]
  fn camera_angle_roundtrip() {
    let mut cam = RaycastCamera::new(66.0);
    let angle = 1.234_f32;
    cam.set_angle(angle);
    assert!(
      (cam.angle() - angle).abs() < EPSILON,
      "angle roundtrip failed: {} != {}",
      cam.angle(),
      angle
    );
  }

  #[test]
  fn door_lifecycle() {
    let mut door = RaycastDoor::new(1, 2, DoorSpeed::Normal);

    // Starts closed
    assert!(matches!(door.state, DoorState::Closed));
    assert_eq!(door.open_amount, 0.0);

    // Activate -> Opening
    door.activate();
    assert!(matches!(door.state, DoorState::Opening));

    // Tick halfway
    let amt = door.tick(0.5);
    assert!((amt - 0.5).abs() < EPSILON);
    assert!(matches!(door.state, DoorState::Opening));

    // Tick to fully open
    let amt = door.tick(0.6);
    assert!((amt - 1.0).abs() < EPSILON);
    assert!(matches!(door.state, DoorState::Open { .. }));

    // Tick through open duration (3.0 seconds)
    door.tick(2.9);
    assert!(matches!(door.state, DoorState::Open { .. }));
    door.tick(0.2); // total 3.1 >= 3.0
    assert!(matches!(door.state, DoorState::Closing));

    // Tick to fully closed
    let amt = door.tick(1.5);
    assert!((amt - 0.0).abs() < EPSILON);
    assert!(matches!(door.state, DoorState::Closed));
  }

  #[test]
  fn door_activate_while_open_is_noop() {
    let mut door = RaycastDoor::new(0, 0, DoorSpeed::Fast);
    door.activate();
    door.tick(1.0); // fully open
    assert!(matches!(door.state, DoorState::Open { .. }));

    // Activating again should be a no-op
    door.activate();
    assert!(matches!(door.state, DoorState::Open { .. }));
  }

  #[test]
  fn door_activate_while_closing_is_noop() {
    let mut door = RaycastDoor::new(0, 0, DoorSpeed::Normal);
    door.activate();
    door.tick(1.0); // fully open
    door.tick(3.5); // closing
    assert!(matches!(door.state, DoorState::Closing));

    door.activate();
    assert!(matches!(door.state, DoorState::Closing));
  }

  #[test]
  fn door_speed_custom() {
    assert_eq!(DoorSpeed::Custom(3.5).cells_per_second(), 3.5);
  }

  #[test]
  fn sprite_defaults() {
    let sprite = RaycastSprite::new(5);
    assert_eq!(sprite.texture_id, 5);
    assert_eq!(sprite.scale, (1.0, 1.0));
    assert!(!sprite.directional);
    assert_eq!(sprite.num_angles, 1);
    assert_eq!(sprite.v_offset, 0.0);
  }

  #[test]
  fn light_defaults() {
    let light = RaycastLight::new(8.0, (1.0, 0.8, 0.6), 2.0);
    assert_eq!(light.radius, 8.0);
    assert_eq!(light.color, (1.0, 0.8, 0.6));
    assert_eq!(light.intensity, 2.0);
    assert!(!light.flicker);
    assert_eq!(light.height, 0.5);
    assert_eq!(light.falloff, 2.0);
  }
}
