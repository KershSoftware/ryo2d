//! The context a scene's `render` callback draws GPU sprites through.

use super::{BatchCollector, CameraView, SpriteGpuRenderer, SpriteInstance};
use crate::core::asset::SpriteHandle;
use crate::core::ecs::{ShaderHandle, ShaderParams, World};

/// Context for GPU sprite rendering.
///
/// This context is passed to the optional `Scene::render` callback,
/// allowing games to use GPU-accelerated sprite batching for
/// high-performance particle systems and effects.
///
/// # Example
///
/// ```ignore
/// fn render(&mut self, gpu: &mut GpuRenderContext) {
///     for particle in &self.particles {
///         let instance = SpriteInstance::new()
///             .with_transform(particle.x, particle.y, particle.rotation, 1.0, 1.0);
///         // The texture is uploaded the first time a frame draws it
///         gpu.add_sprite_instance(&particle.sprite, instance, particle.z_order, false);
///     }
/// }
/// ```
pub struct GpuRenderContext<'a> {
  /// The batch collector for grouping sprites by texture
  pub batch: &'a mut BatchCollector,
  /// The GPU sprite renderer (for uploading textures)
  gpu_renderer: &'a mut SpriteGpuRenderer,
  /// Device for compiling game shaders
  device: &'a wgpu::Device,
  /// The main camera's view this frame
  camera: &'a CameraView,
  /// The size of screen space
  screen: glam::Vec2,
  /// The ECS world (read-only for rendering)
  pub world: &'a World,
}

impl<'a> GpuRenderContext<'a> {
  /// Create a new GPU render context
  pub fn new(
    batch: &'a mut BatchCollector,
    gpu_renderer: &'a mut SpriteGpuRenderer,
    device: &'a wgpu::Device,
    camera: &'a CameraView,
    screen: glam::Vec2,
    world: &'a World,
  ) -> Self {
    Self {
      batch,
      gpu_renderer,
      device,
      camera,
      screen,
      world,
    }
  }

  /// Add a sprite drawn with a game shader.
  ///
  /// `params` are matched by name to the fields of the shader's `Params`
  /// struct (the sprite's own effect parameters are overwritten with them). A
  /// shader that fails to compile is logged and the sprite is drawn plain.
  pub fn add_custom_sprite(
    &mut self,
    sprite: &SpriteHandle,
    shader: &ShaderHandle,
    params: &ShaderParams,
    mut instance: SpriteInstance,
    z_order: i32,
  ) {
    let shading = match self
      .gpu_renderer
      .shade_custom(self.device, shader, params, &mut instance)
    {
      Some(key) => crate::core::render::SpriteShading::Custom(key),
      None => crate::core::render::SpriteShading::Plain,
    };
    self.batch.add_shaded(sprite, shading, instance, z_order);
  }

  /// Get screen dimensions (width, height)
  pub fn screen_size(&self) -> (f32, f32) {
    (self.screen.x, self.screen.y)
  }

  /// Get screen width
  pub fn screen_width(&self) -> u32 {
    self.screen.x as u32
  }

  /// Get screen height
  pub fn screen_height(&self) -> u32 {
    self.screen.y as u32
  }

  /// The main camera's view this frame (`world_to_screen`, `center`,
  /// `scale`, `bounds`).
  pub fn camera(&self) -> &'a CameraView {
    self.camera
  }

  /// Add a sprite instance drawing `sprite`'s texture. Instances are in
  /// world space (their transform's centre is a world position); for
  /// something fixed on screen, place it with `camera().screen_to_world`.
  ///
  /// The texture is uploaded the first time a frame draws it, and dropped
  /// once no frame has drawn it for 600 frames. This is the low-level API
  /// for GPU sprites; most games use the higher-level ones (particles,
  /// `GpuParallaxBackground`).
  pub fn add_sprite_instance(
    &mut self,
    sprite: &SpriteHandle,
    instance: SpriteInstance,
    z_order: i32,
    has_effects: bool,
  ) {
    self.batch.add(sprite, has_effects, instance, z_order);
  }
}
