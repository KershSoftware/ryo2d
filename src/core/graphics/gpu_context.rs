//! The context a scene's `render` callback draws GPU sprites through.

use super::{BatchCollector, Camera, SpriteGpuRenderer, SpriteInstance};
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
///         // Upload texture if not cached
///         gpu.upload_sprite(&particle.sprite);
///
///         // Create instance with transform
///         let instance = SpriteInstance::new()
///             .with_transform(particle.x, particle.y, particle.rotation, 1.0, 1.0);
///
///         // Add to batch
///         gpu.batch.add(particle.sprite.id(), false, instance, particle.z_order);
///     }
/// }
/// ```
pub struct GpuRenderContext<'a> {
  /// The batch collector for grouping sprites by texture
  pub batch: &'a mut BatchCollector,
  /// The GPU sprite renderer (for uploading textures)
  gpu_renderer: &'a mut SpriteGpuRenderer,
  /// Device for GPU operations
  device: &'a wgpu::Device,
  /// Queue for GPU operations
  queue: &'a wgpu::Queue,
  /// Camera for coordinate transforms
  pub camera: &'a Camera,
  /// The ECS world (read-only for rendering)
  pub world: &'a World,
}

impl<'a> GpuRenderContext<'a> {
  /// Create a new GPU render context
  pub fn new(
    batch: &'a mut BatchCollector,
    gpu_renderer: &'a mut SpriteGpuRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    camera: &'a Camera,
    world: &'a World,
  ) -> Self {
    Self {
      batch,
      gpu_renderer,
      device,
      queue,
      camera,
      world,
    }
  }

  /// Upload a sprite texture to the GPU cache.
  ///
  /// Call this for each unique sprite, every frame, before adding its
  /// instances to the batch. Sprites are cached by ID, so later calls with
  /// the same sprite are no-ops; textures neither uploaded nor drawn for 600
  /// frames are dropped.
  pub fn upload_sprite(&mut self, sprite: &SpriteHandle) {
    self
      .gpu_renderer
      .upload_sprite(self.device, self.queue, sprite);
  }

  /// Check if a sprite texture is already uploaded
  pub fn has_texture(&self, id: &str) -> bool {
    self.gpu_renderer.has_texture(id)
  }

  /// Add a sprite drawn with a game shader.
  ///
  /// `texture_id` is the id the texture was uploaded under. `params` are
  /// matched by name to the fields of the shader's `Params` struct (the
  /// sprite's own effect parameters are overwritten with them). A shader that
  /// fails to compile is logged and the sprite is drawn plain.
  pub fn add_custom_sprite(
    &mut self,
    texture_id: &str,
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
    self
      .batch
      .add_shaded(texture_id, shading, instance, z_order);
  }

  /// Get screen dimensions (width, height)
  pub fn screen_size(&self) -> (f32, f32) {
    self.camera.viewport_size()
  }

  /// Get screen width
  pub fn screen_width(&self) -> u32 {
    let (w, _) = self.camera.viewport_size();
    w as u32
  }

  /// Get screen height
  pub fn screen_height(&self) -> u32 {
    let (_, h) = self.camera.viewport_size();
    h as u32
  }

  /// Get the camera reference
  pub fn camera(&self) -> &Camera {
    self.camera
  }

  /// Add a sprite instance to the batch collector.
  ///
  /// This is the low-level API for adding GPU sprites. Most games should use
  /// the higher-level APIs (e.g., GpuParallaxBackground) instead.
  pub fn add_sprite_instance(
    &mut self,
    texture_id: &str,
    instance: SpriteInstance,
    z_order: i32,
    has_effects: bool,
  ) {
    self.batch.add(texture_id, has_effects, instance, z_order);
  }
}
