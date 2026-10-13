//! Per-camera GPU uniforms: each camera's view-projection, image size,
//! clocks and pixel snapping, one buffer per camera entity.

use std::collections::HashMap;

use crate::core::ecs::Entity;

use super::CameraView;

/// What the sprite (and later lighting and trail) shaders read for the
/// camera they draw through. Matches `Globals` in `sprite.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct CameraGlobals {
  view_proj: [[f32; 4]; 4],
  /// The camera image's size in pixels
  resolution: [f32; 2],
  /// Game time (sprite effects and custom sprite shaders animate on it)
  game_time: f32,
  real_time: f32,
  /// 1.0 when axis-aligned sprites snap to whole pixels (pixel art)
  snap: f32,
  _pad: [f32; 3],
}

impl CameraGlobals {
  pub(crate) fn new(
    view: &CameraView,
    image_size: (u32, u32),
    game_time: f32,
    real_time: f32,
    snap: bool,
  ) -> Self {
    Self {
      view_proj: view.view_proj.to_cols_array_2d(),
      resolution: [image_size.0 as f32, image_size.1 as f32],
      game_time,
      real_time,
      snap: if snap { 1.0 } else { 0.0 },
      _pad: [0.0; 3],
    }
  }
}

/// One uniform buffer per camera entity, kept across frames.
#[derive(Default)]
pub(crate) struct CameraUniforms {
  buffers: HashMap<Entity, wgpu::Buffer>,
}

impl CameraUniforms {
  /// Write `cameras`' globals, creating buffers for new cameras and
  /// dropping those of cameras that are gone.
  pub(crate) fn sync(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    cameras: &[(Entity, CameraGlobals)],
  ) {
    self
      .buffers
      .retain(|entity, _| cameras.iter().any(|(camera, _)| camera == entity));
    for (camera, globals) in cameras {
      let buffer = self.buffers.entry(*camera).or_insert_with(|| {
        device.create_buffer(&wgpu::BufferDescriptor {
          label: Some("Camera Globals"),
          size: std::mem::size_of::<CameraGlobals>() as u64,
          usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
          mapped_at_creation: false,
        })
      });
      queue.write_buffer(buffer, 0, bytemuck::bytes_of(globals));
    }
  }

  /// The buffer of `camera`, if it was synced.
  pub(crate) fn buffer(&self, camera: Entity) -> Option<&wgpu::Buffer> {
    self.buffers.get(&camera)
  }

  /// The cameras with a buffer.
  pub(crate) fn cameras(&self) -> impl Iterator<Item = Entity> + '_ {
    self.buffers.keys().copied()
  }

  #[cfg(test)]
  pub(crate) fn len(&self) -> usize {
    self.buffers.len()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn camera_globals_are_96_bytes() {
    assert_eq!(std::mem::size_of::<CameraGlobals>(), 96);
  }

  #[test]
  fn uniforms_follow_the_live_cameras() {
    let Ok((device, queue)) = crate::core::testing::shared_gpu() else {
      return;
    };
    let mut world = crate::core::ecs::World::new();
    let (a, b) = (world.spawn_empty().id(), world.spawn_empty().id());
    let globals = CameraGlobals::new(&CameraView::default(), (8, 8), 0.0, 0.0, false);
    let mut uniforms = CameraUniforms::default();
    uniforms.sync(&device, &queue, &[(a, globals), (b, globals)]);
    assert_eq!(uniforms.len(), 2);
    uniforms.sync(&device, &queue, &[(b, globals)]);
    assert_eq!(uniforms.len(), 1);
    assert!(uniforms.buffer(a).is_none() && uniforms.buffer(b).is_some());
  }
}
