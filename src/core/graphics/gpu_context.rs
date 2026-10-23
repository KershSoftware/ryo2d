//! The context a scene's `render` callback draws GPU sprites through.

use super::{BatchCollector, CameraView, Image, SpriteDraw, SpriteGpuRenderer, SpriteInstance};
use crate::core::asset::Handle;
use crate::core::ecs::{Shader, ShaderParams, World, shaders_of};

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
///         let draw = SpriteDraw { z: particle.z_order, ..Default::default() };
///         gpu.add_sprite(&particle.image, instance, draw);
///     }
/// }
/// ```
pub struct GpuRenderContext<'a> {
  /// The batch collector for grouping sprites by texture
  pub batch: &'a mut BatchCollector,
  /// The GPU sprite renderer (for uploading textures)
  gpu_renderer: &'a mut SpriteGpuRenderer,
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
    camera: &'a CameraView,
    screen: glam::Vec2,
    world: &'a World,
  ) -> Self {
    Self {
      batch,
      gpu_renderer,
      camera,
      screen,
      world,
    }
  }

  /// Add a sprite drawn with a game shader.
  ///
  /// `params` are matched by name to the fields of the shader's `Params`
  /// struct (the sprite's own effect parameters are overwritten with them). A
  /// shader that is not loaded draws the sprite plain, as one that fails to
  /// compile does (that one is logged once per version).
  pub fn add_custom_sprite(
    &mut self,
    image: &Handle<Image>,
    shader: &Handle<Shader>,
    params: &ShaderParams,
    mut instance: SpriteInstance,
    draw: SpriteDraw,
  ) {
    let shaders = shaders_of(self.world);
    let shading = match self
      .gpu_renderer
      .shade_custom(shader.id(), shaders, params, &mut instance)
    {
      Some(key) => crate::core::render::SpriteShading::Custom(key),
      None => crate::core::render::SpriteShading::Plain,
    };
    self.batch.add_shaded(image.id(), shading, instance, draw);
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

  /// Add a sprite instance drawing `image`, as `draw` says (z, blend,
  /// filter, lighting, effects). Instances are in world space (their
  /// transform's centre is a world position); for something fixed on
  /// screen, place it with `camera().screen_to_world`.
  ///
  /// The texture is uploaded the first time a frame draws it, and freed with
  /// the image's last handle. An image that is not loaded draws the checker
  /// in debug builds and nothing in release ones. This is the low-level API
  /// for GPU sprites; most games use the higher-level ones (particles,
  /// `ParallaxLayer`).
  pub fn add_sprite(&mut self, image: &Handle<Image>, instance: SpriteInstance, draw: SpriteDraw) {
    self.batch.add(image, instance, draw);
  }
}
