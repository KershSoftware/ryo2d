//! GPU Parallax rendering pass
//!
//! This pass renders parallax background layers on the GPU using instanced
//! sprite rendering. This is much faster than CPU rendering for large images.

use crate::core::graphics::parallax::TileMode;
use crate::core::graphics::sprite_gpu::SpriteInstance;
use crate::core::render::pass_ids;
use crate::core::render::{RenderContext, RenderPass, RenderPassId};

/// A parallax layer configured for GPU rendering
#[derive(Debug, Clone)]
pub struct GpuParallaxLayer {
  /// Texture ID (must be pre-uploaded to SpriteGpuRenderer cache)
  pub texture_id: String,
  /// Parallax factor (0.0 = static, 1.0 = moves with camera)
  pub scroll_speed: f32,
  /// Auto-scroll speed in pixels per second
  pub auto_scroll_speed: f32,
  /// Vertical offset from top of screen
  pub y_offset: f32,
  /// Z-order for sorting with other sprites
  pub z_order: i32,
  /// How the layer tiles horizontally
  pub tile_mode: TileMode,
  /// Sprite dimensions (for tiling calculations)
  pub sprite_width: u32,
  pub sprite_height: u32,
  /// Optional scale factor
  pub scale: f32,
  /// Tint color (RGBA, 0-255)
  pub tint: [u8; 4],
}

impl GpuParallaxLayer {
  /// Create a new GPU parallax layer
  pub fn new(
    texture_id: impl Into<String>,
    sprite_width: u32,
    sprite_height: u32,
    scroll_speed: f32,
    z_order: i32,
  ) -> Self {
    Self {
      texture_id: texture_id.into(),
      scroll_speed,
      auto_scroll_speed: 0.0,
      y_offset: 0.0,
      z_order,
      tile_mode: TileMode::None,
      sprite_width,
      sprite_height,
      scale: 1.0,
      tint: [255, 255, 255, 255],
    }
  }

  /// Set auto-scroll speed
  pub fn with_auto_scroll(mut self, speed: f32) -> Self {
    self.auto_scroll_speed = speed;
    self
  }

  /// Set vertical offset
  pub fn with_y_offset(mut self, offset: f32) -> Self {
    self.y_offset = offset;
    self
  }

  /// Set tile mode
  pub fn with_tile_mode(mut self, mode: TileMode) -> Self {
    self.tile_mode = mode;
    self
  }

  /// Set scale factor
  pub fn with_scale(mut self, scale: f32) -> Self {
    self.scale = scale;
    self
  }

  /// Set tint color
  pub fn with_tint(mut self, r: u8, g: u8, b: u8, a: u8) -> Self {
    self.tint = [r, g, b, a];
    self
  }

  /// Compute sprite instances for this layer
  fn compute_instances(
    &self,
    screen_width: u32,
    _screen_height: u32, // Reserved for future vertical culling
    camera_x: f32,
    elapsed_time: f32,
  ) -> Vec<SpriteInstance> {
    let scaled_width = self.sprite_width as f32 * self.scale;
    let scaled_height = self.sprite_height as f32 * self.scale;

    if scaled_width <= 0.0 || scaled_height <= 0.0 {
      return vec![];
    }

    // Calculate total offset: camera-based parallax + time-based auto-scroll
    let camera_offset = camera_x * self.scroll_speed;
    let auto_offset = elapsed_time * self.auto_scroll_speed;
    let total_offset = (camera_offset + auto_offset) * self.scale;

    let y_pos = self.y_offset * self.scale;

    match self.tile_mode {
      TileMode::None => {
        // Single sprite instance
        let x_pos = -total_offset;
        vec![self.create_instance(x_pos, y_pos, scaled_width, scaled_height)]
      }

      TileMode::Horizontal => {
        // Tile to fill screen width
        let tiles_needed = (screen_width as f32 / scaled_width).ceil() as i32 + 1;
        let mut instances = Vec::with_capacity(tiles_needed as usize);

        for i in 0..tiles_needed {
          let x_pos = (i as f32 * scaled_width) - total_offset;
          // Only include if at least partially visible
          if x_pos + scaled_width >= 0.0 && x_pos < screen_width as f32 {
            instances.push(self.create_instance(x_pos, y_pos, scaled_width, scaled_height));
          }
        }

        instances
      }

      TileMode::InfiniteScroll => {
        // Infinite scrolling with seamless wrapping
        let tiles_needed = (screen_width as f32 / scaled_width).ceil() as i32 + 2;
        let mut instances = Vec::with_capacity(tiles_needed as usize);

        // Calculate base offset with modulo for seamless wrapping
        let base_offset = total_offset.rem_euclid(scaled_width);

        for i in 0..tiles_needed {
          let screen_x = (i as f32 * scaled_width) - base_offset - scaled_width;
          // Only include if at least partially visible
          if screen_x + scaled_width >= 0.0 && screen_x < screen_width as f32 {
            instances.push(self.create_instance(screen_x, y_pos, scaled_width, scaled_height));
          }
        }

        instances
      }
    }
  }

  /// Create a sprite instance at the given position
  fn create_instance(&self, x: f32, y: f32, width: f32, height: f32) -> SpriteInstance {
    SpriteInstance::new()
      .with_transform(
        x + width / 2.0,  // Center x
        y + height / 2.0, // Center y
        0.0,              // No rotation
        width,            // Scale x = width
        height,           // Scale y = height
      )
      .with_tint(
        crate::core::ui::Color::new(self.tint[0], self.tint[1], self.tint[2], self.tint[3]),
        1.0,
      )
  }
}

/// Render pass for GPU-accelerated parallax backgrounds.
///
/// This pass renders parallax layers using GPU instancing, which is much
/// faster than CPU rendering for large images or many layers.
///
/// # Usage
///
/// ```ignore
/// let mut parallax = ParallaxPass::new();
///
/// // Add layers (must have textures pre-uploaded to sprite_gpu)
/// parallax.add_layer(GpuParallaxLayer::new("sky", 1920, 1080, 0.0, -100));
/// parallax.add_layer(
///     GpuParallaxLayer::new("mountains", 1920, 1080, 0.2, -80)
///         .with_tile_mode(TileMode::InfiniteScroll)
///         .with_auto_scroll(10.0)
/// );
///
/// // Add to pipeline
/// pipeline.add_pass(parallax);
/// ```
pub struct ParallaxPass {
  /// Parallax layers to render
  layers: Vec<GpuParallaxLayer>,
  /// Whether this pass is enabled
  enabled: bool,
  /// Elapsed time tracker (updated each frame)
  elapsed_time: f32,
}

impl Default for ParallaxPass {
  fn default() -> Self {
    Self::new()
  }
}

impl ParallaxPass {
  /// Create an empty parallax pass
  pub fn new() -> Self {
    Self {
      layers: Vec::new(),
      enabled: true,
      elapsed_time: 0.0,
    }
  }

  /// Add a layer
  pub fn add_layer(&mut self, layer: GpuParallaxLayer) {
    self.layers.push(layer);
    // Keep layers sorted by z_order
    self.layers.sort_by_key(|l| l.z_order);
  }

  /// Remove a layer by texture ID
  pub fn remove_layer(&mut self, texture_id: &str) -> Option<GpuParallaxLayer> {
    if let Some(pos) = self.layers.iter().position(|l| l.texture_id == texture_id) {
      Some(self.layers.remove(pos))
    } else {
      None
    }
  }

  /// Clear all layers
  pub fn clear(&mut self) {
    self.layers.clear();
  }

  /// Get the number of layers
  pub fn layer_count(&self) -> usize {
    self.layers.len()
  }

  /// Update elapsed time (for auto-scrolling)
  pub fn update(&mut self, delta_time: f32) {
    self.elapsed_time += delta_time;
  }

  /// Set elapsed time directly
  pub fn set_elapsed_time(&mut self, time: f32) {
    self.elapsed_time = time;
  }

  /// Get a mutable reference to a layer by texture ID
  pub fn get_layer_mut(&mut self, texture_id: &str) -> Option<&mut GpuParallaxLayer> {
    self.layers.iter_mut().find(|l| l.texture_id == texture_id)
  }

  /// Get all layers
  pub fn layers(&self) -> &[GpuParallaxLayer] {
    &self.layers
  }
}

impl RenderPass for ParallaxPass {
  fn id(&self) -> RenderPassId {
    pass_ids::PARALLAX
  }

  fn dependencies(&self) -> &[RenderPassId] {
    &[pass_ids::CLEAR]
  }

  fn execute(&mut self, ctx: &mut RenderContext) {
    if self.layers.is_empty() {
      return;
    }

    // Update elapsed time from context
    self.elapsed_time = ctx.time;

    // Collect all instances for batching
    for layer in &self.layers {
      // Check if texture is cached
      if !ctx.sprite_gpu.has_texture(&layer.texture_id) {
        continue;
      }

      let instances =
        layer.compute_instances(ctx.width, ctx.height, ctx.camera.x, self.elapsed_time);

      if instances.is_empty() {
        continue;
      }

      // Add to batch collector
      for instance in instances {
        ctx
          .batch_collector
          .add(&layer.texture_id, false, instance, layer.z_order);
      }
    }

    // Render all parallax batches
    // Note: The actual render_batches call happens in a later pass (GpuSpritePass)
    // since parallax instances are mixed with regular GPU sprites in the batch collector
  }

  fn should_execute(&self, _ctx: &RenderContext) -> bool {
    !self.layers.is_empty()
  }

  fn is_enabled(&self) -> bool {
    self.enabled
  }

  fn set_enabled(&mut self, enabled: bool) {
    self.enabled = enabled;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_layer_creation() {
    let layer = GpuParallaxLayer::new("test", 100, 100, 0.5, -10);
    assert_eq!(layer.texture_id, "test");
    assert_eq!(layer.scroll_speed, 0.5);
    assert_eq!(layer.z_order, -10);
  }

  #[test]
  fn test_layer_builder() {
    let layer = GpuParallaxLayer::new("test", 100, 100, 0.5, -10)
      .with_auto_scroll(10.0)
      .with_y_offset(50.0)
      .with_tile_mode(TileMode::InfiniteScroll)
      .with_scale(2.0);

    assert_eq!(layer.auto_scroll_speed, 10.0);
    assert_eq!(layer.y_offset, 50.0);
    assert_eq!(layer.tile_mode, TileMode::InfiniteScroll);
    assert_eq!(layer.scale, 2.0);
  }

  #[test]
  fn test_pass_creation() {
    let pass = ParallaxPass::new();
    assert_eq!(pass.layer_count(), 0);
    assert!(pass.is_enabled());
  }

  #[test]
  fn test_add_remove_layer() {
    let mut pass = ParallaxPass::new();
    pass.add_layer(GpuParallaxLayer::new("layer1", 100, 100, 0.0, -10));
    pass.add_layer(GpuParallaxLayer::new("layer2", 100, 100, 0.5, -5));
    assert_eq!(pass.layer_count(), 2);

    let removed = pass.remove_layer("layer1");
    assert!(removed.is_some());
    assert_eq!(pass.layer_count(), 1);
  }

  #[test]
  fn test_layer_sorting() {
    let mut pass = ParallaxPass::new();
    pass.add_layer(GpuParallaxLayer::new("front", 100, 100, 0.8, -5));
    pass.add_layer(GpuParallaxLayer::new("back", 100, 100, 0.2, -20));
    pass.add_layer(GpuParallaxLayer::new("mid", 100, 100, 0.5, -10));

    let layers = pass.layers();
    assert_eq!(layers[0].texture_id, "back");
    assert_eq!(layers[1].texture_id, "mid");
    assert_eq!(layers[2].texture_id, "front");
  }

  #[test]
  fn test_compute_instances_none() {
    let layer = GpuParallaxLayer::new("test", 100, 100, 0.0, 0);
    let instances = layer.compute_instances(800, 600, 0.0, 0.0);
    assert_eq!(instances.len(), 1);
  }

  #[test]
  fn test_compute_instances_horizontal() {
    let layer =
      GpuParallaxLayer::new("test", 100, 100, 0.0, 0).with_tile_mode(TileMode::Horizontal);
    let instances = layer.compute_instances(800, 600, 0.0, 0.0);
    // 800 / 100 = 8, + 1 buffer = 9
    assert!(instances.len() >= 8);
  }

  #[test]
  fn test_compute_instances_infinite_scroll() {
    let layer =
      GpuParallaxLayer::new("test", 100, 100, 0.0, 0).with_tile_mode(TileMode::InfiniteScroll);
    let instances = layer.compute_instances(800, 600, 0.0, 0.0);
    // Should have enough tiles to cover screen with buffer
    assert!(instances.len() >= 8);
  }
}
