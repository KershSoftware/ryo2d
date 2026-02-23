//! Systems and GPU-ready uniform structs for the raycasting renderer.
//!
//! The systems are pure functions designed to be called from the game loop.
//! The uniform structs are `#[repr(C)]` / `bytemuck`-friendly so they can be
//! uploaded directly to GPU constant buffers.

use super::components::RaycastCamera;
use super::components::RaycastDoor;
use super::world::RaycastWorld;

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Update all door animations and sync state back to the world map.
///
/// Each tuple contains a `RaycastDoor` component. The system calls
/// [`RaycastDoor::tick`] and writes the resulting open amount into the
/// corresponding [`MapCell`](super::world::MapCell).
pub fn door_system(doors: &mut [(RaycastDoor,)], world: &mut RaycastWorld, dt: f32) {
  for (door,) in doors.iter_mut() {
    let open_amount = door.tick(dt);
    if let Some(cell) = world.get_mut(door.map_x as usize, door.map_y as usize) {
      cell.door_state = open_amount;
    }
  }
}

/// Sync camera direction/plane from a look angle (in radians).
pub fn camera_follow_system(camera: &mut RaycastCamera, angle: f32) {
  camera.set_angle(angle);
}

// ---------------------------------------------------------------------------
// GPU Uniform Structs
// ---------------------------------------------------------------------------

/// Camera parameters uploaded as a GPU uniform buffer.
///
/// Total size: 48 bytes (12 × f32).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
  /// Camera world position (x, y).
  pub pos: [f32; 2],
  /// Unit look direction vector.
  pub dir: [f32; 2],
  /// Camera plane vector (perpendicular to dir, length = tan(fov/2)).
  pub plane: [f32; 2],
  /// Screen dimensions in pixels (width, height).
  pub screen_size: [f32; 2],
  /// Vertical eye height (0.5 = eye level).
  pub height: f32,
  /// Time since start in seconds (for animated effects).
  pub time: f32,
  /// Padding to reach 48 bytes.
  pub _pad: [f32; 2],
}

/// Map metadata uploaded as a GPU uniform buffer.
///
/// Total size: 32 bytes (8 × 4 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MapInfoUniform {
  /// Map dimensions in cells (width, height).
  pub dimensions: [u32; 2],
  /// Maximum ray marching distance in cells.
  pub max_distance: f32,
  /// Fog density (0.0 = no fog).
  pub fog_density: f32,
  /// Fog colour (r, g, b).
  pub fog_color: [f32; 3],
  /// Padding to reach 32 bytes.
  pub _pad: f32,
}

/// Per-sprite instance data for GPU instanced rendering.
///
/// Total size: 32 bytes (8 × 4 bytes).
///
/// Field order matters: `vec2<f32>` in WGSL requires 8-byte alignment,
/// so all `[f32; 2]` fields must sit on 8-byte boundaries to avoid
/// implicit padding mismatches between Rust and WGSL.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SpriteInstance {
  /// World position (x, y).
  pub pos: [f32; 2],
  /// Scale in cells (width, height).
  pub scale: [f32; 2],
  /// Texture array index.
  pub texture_id: u32,
  /// Vertical offset from the floor in world units.
  pub v_offset: f32,
  /// Padding to reach 32 bytes.
  pub _pad: [f32; 2],
}

/// Per-light instance data for GPU lighting passes.
///
/// Total size: 32 bytes (8 × 4 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightInstance {
  /// World position (x, y).
  pub pos: [f32; 2],
  /// Light radius in cells.
  pub radius: f32,
  /// Brightness multiplier.
  pub intensity: f32,
  /// Light colour (r, g, b).
  pub color: [f32; 3],
  /// Padding to reach 32 bytes.
  pub _pad: f32,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::raycast::components::*;
  use crate::core::raycast::world::*;

  #[test]
  fn door_system_updates_world() {
    let mut world = RaycastWorld::new(8, 8);
    // Set cell at (3,4) as a door
    world.set(
      3,
      4,
      MapCell {
        flags: MapFlags::DOOR,
        wall_texture: 1,
        ..MapCell::default()
      },
    );
    let mut doors = vec![(RaycastDoor::new(3, 4, DoorSpeed::Custom(2.0)),)];
    doors[0].0.activate();
    door_system(&mut doors, &mut world, 0.25);
    let cell = world.get(3, 4).unwrap();
    assert!(
      (cell.door_state - 0.5).abs() < 0.01,
      "expected door_state ~0.5, got {}",
      cell.door_state
    );
  }

  #[test]
  fn camera_follow_system_sets_angle() {
    let mut cam = RaycastCamera::new(66.0);
    let target = std::f32::consts::FRAC_PI_2;
    camera_follow_system(&mut cam, target);
    assert!(
      (cam.angle() - target).abs() < 1e-5,
      "expected angle ~{}, got {}",
      target,
      cam.angle()
    );
  }

  #[test]
  fn camera_uniform_size() {
    assert_eq!(std::mem::size_of::<CameraUniform>(), 48);
  }

  #[test]
  fn map_info_uniform_size() {
    assert_eq!(std::mem::size_of::<MapInfoUniform>(), 32);
  }

  #[test]
  fn sprite_instance_size() {
    assert_eq!(std::mem::size_of::<SpriteInstance>(), 32);
  }

  #[test]
  fn light_instance_size() {
    assert_eq!(std::mem::size_of::<LightInstance>(), 32);
  }
}
