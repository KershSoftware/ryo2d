//! GPU-accelerated raycasting for pseudo-3D rendering (Wolfenstein/Build engine style).

mod components;
mod gpu;
mod systems;

mod world;

// Future exports (uncomment as modules are implemented):
pub use components::{
  DoorSpeed, DoorState, RaycastCamera, RaycastDoor, RaycastLight, RaycastSprite,
};
pub use gpu::GpuRaycastRenderer;
pub use systems::{
  CameraUniform, LightInstance, MapInfoUniform, SpriteInstance, camera_follow_system, door_system,
};
pub use world::{MapCell, MapFlags, RaycastWorld};
