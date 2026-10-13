//! GPU-accelerated parallax background rendering
//!
//! Parallax layers are batched GPU sprites: a scene builds a
//! [`GpuParallaxBackground`] and adds it to the frame from [`Scene::render`],
//! where its layers sort by z with entities and other sprites.
//!
//! [`Scene::render`]: crate::core::scene::Scene::render
//!
//! # Example
//!
//! ```ignore
//! // In on_enter:
//! let parallax = GpuParallaxBuilder::new()
//!     .static_layer(&sky_sprite, -100)
//!     .layer(&mountains_sprite, 0.3, -80)
//!         .with_tile_mode(TileMode::InfiniteScroll)
//!         .with_auto_scroll(10.0)
//!     .build();
//!
//! // In Scene::render (textures upload automatically):
//! fn render(&mut self, gpu: &mut GpuRenderContext) {
//!     self.parallax.render(gpu);
//! }
//! ```

use super::GpuRenderContext;
use crate::core::asset::SpriteHandle;
use crate::core::graphics::CameraView;
use crate::core::graphics::parallax::TileMode;
use crate::core::graphics::sprite_gpu::SpriteInstance;
use glam::Vec2;

/// One layer of a [`GpuParallaxBackground`], drawn as instanced GPU sprites.
#[derive(Clone)]
pub struct GpuParallaxLayer {
  /// The layer's image
  sprite: SpriteHandle,
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
  sprite_width: u32,
  sprite_height: u32,
  /// Scale factor
  scale: f32,
  /// Tint color (RGBA)
  tint: [u8; 4],
}

impl GpuParallaxLayer {
  /// Create a new GPU parallax layer from a sprite handle.
  pub fn new(sprite: &SpriteHandle, scroll_speed: f32, z_order: i32) -> Self {
    sprite.with_sprite(|s| Self {
      sprite: sprite.clone(),
      scroll_speed,
      auto_scroll_speed: 0.0,
      y_offset: 0.0,
      z_order,
      tile_mode: TileMode::None,
      sprite_width: s.width(),
      sprite_height: s.height(),
      scale: 1.0,
      tint: [255, 255, 255, 255],
    })
  }

  /// Set auto-scroll speed in pixels per second
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

  /// The layer's image
  pub fn sprite(&self) -> &SpriteHandle {
    &self.sprite
  }

  /// How far the layer moves with the camera: 0 stays on screen, 1 stays in
  /// the world.
  pub fn with_scroll_speed(mut self, speed: f32) -> Self {
    self.scroll_speed = speed;
    self
  }

  /// A layer of a solid `width`x`height` sprite (tests).
  #[cfg(test)]
  pub(crate) fn test_layer(width: u32, height: u32) -> Self {
    let sprite = SpriteHandle::from_sprite(
      crate::core::graphics::Sprite::solid_rect(width, height, 255, 255, 255, 255),
      "parallax-test",
    );
    Self::new(&sprite, 0.0, 0)
  }

  /// The layer's sprites as world-space instances: laid out in `view`'s
  /// screen space (scrolled by the camera's centre times the scroll speed,
  /// on both axes, and by the auto-scroll), then taken into the world.
  fn world_instances(&self, view: &CameraView, elapsed_time: f32) -> Vec<SpriteInstance> {
    let scale = self.scale * view.scale;
    let scaled_width = self.sprite_width as f32 * scale;
    let scaled_height = self.sprite_height as f32 * scale;
    if !(scaled_width > 0.0 && scaled_height > 0.0) {
      return vec![];
    }

    // Camera-based parallax plus time-based auto-scroll, in screen units
    let camera = view.center * self.scroll_speed;
    let total_offset = (camera.x + elapsed_time * self.auto_scroll_speed) * scale;
    let y_pos = (self.y_offset - camera.y) * scale;
    let screen_width = view.rect_size.x;

    let mut corners = Vec::new();
    match self.tile_mode {
      TileMode::None => corners.push(Vec2::new(-total_offset, y_pos)),
      TileMode::Horizontal => {
        let tiles_needed = (screen_width / scaled_width).ceil() as i32 + 1;
        for i in 0..tiles_needed {
          let x_pos = (i as f32 * scaled_width) - total_offset;
          if x_pos + scaled_width >= 0.0 && x_pos < screen_width {
            corners.push(Vec2::new(x_pos, y_pos));
          }
        }
      }
      TileMode::InfiniteScroll => {
        let tiles_needed = (screen_width / scaled_width).ceil() as i32 + 2;
        let base_offset = total_offset.rem_euclid(scaled_width);
        for i in 0..tiles_needed {
          let x_pos = (i as f32 * scaled_width) - base_offset - scaled_width;
          if x_pos + scaled_width >= 0.0 && x_pos < screen_width {
            corners.push(Vec2::new(x_pos, y_pos));
          }
        }
      }
    }
    let size = Vec2::new(scaled_width, scaled_height);
    corners
      .into_iter()
      .map(|corner| {
        let center = view.screen_to_world(view.rect_min + corner + size / 2.0);
        self.create_instance(center, size / view.scale, view.rotation)
      })
      .collect()
  }

  /// A world-space instance centred on `center`, turned with the camera so
  /// it stays square to the screen.
  fn create_instance(&self, center: Vec2, size: Vec2, rotation: f32) -> SpriteInstance {
    use crate::core::ui::Color;

    SpriteInstance::new()
      .with_transform(center.x, center.y, rotation, size.x, size.y)
      .with_tint(
        Color::new(self.tint[0], self.tint[1], self.tint[2], self.tint[3]),
        1.0,
      )
  }
}

/// GPU-accelerated parallax background.
///
/// This struct manages parallax layers and renders them using GPU instancing.
/// For best performance, use this instead of `ParallaxBackground` when you have
/// large images or many layers.
pub struct GpuParallaxBackground {
  layers: Vec<GpuParallaxLayer>,
  elapsed_time: f32,
}

impl Default for GpuParallaxBackground {
  fn default() -> Self {
    Self::new()
  }
}

impl GpuParallaxBackground {
  /// Create a new empty GPU parallax background
  pub fn new() -> Self {
    Self {
      layers: Vec::new(),
      elapsed_time: 0.0,
    }
  }

  /// Add a layer to the background.
  ///
  /// Layers are automatically sorted by z_order.
  pub fn add_layer(&mut self, layer: GpuParallaxLayer) {
    self.layers.push(layer);
    self.layers.sort_by_key(|l| l.z_order);
  }

  /// Remove the layer drawing `sprite`
  pub fn remove_layer(&mut self, sprite: &SpriteHandle) -> Option<GpuParallaxLayer> {
    if let Some(pos) = self
      .layers
      .iter()
      .position(|l| l.sprite.serial() == sprite.serial())
    {
      Some(self.layers.remove(pos))
    } else {
      None
    }
  }

  /// Get the number of layers
  pub fn layer_count(&self) -> usize {
    self.layers.len()
  }

  /// The layer drawing `sprite`
  pub fn get_layer_mut(&mut self, sprite: &SpriteHandle) -> Option<&mut GpuParallaxLayer> {
    self
      .layers
      .iter_mut()
      .find(|l| l.sprite.serial() == sprite.serial())
  }

  /// Update elapsed time for auto-scrolling.
  ///
  /// Call this every frame with delta time.
  pub fn update(&mut self, delta_time: f32) {
    self.elapsed_time += delta_time;
  }

  /// Set elapsed time directly
  pub fn set_elapsed_time(&mut self, time: f32) {
    self.elapsed_time = time;
  }

  /// Render the parallax background using GPU rendering.
  ///
  /// Call it from [`Scene::render`](crate::core::scene::Scene::render).
  pub fn render(&self, ctx: &mut GpuRenderContext) {
    // Parallax follows the main camera; other cameras see it where it sits
    let view = ctx.camera();
    for layer in &self.layers {
      for instance in layer.world_instances(view, self.elapsed_time) {
        ctx.add_sprite_instance(&layer.sprite, instance, layer.z_order, false);
      }
    }
  }
}

/// Builder for creating GPU parallax backgrounds with a fluent API.
///
/// # Example
///
/// ```ignore
/// let parallax = GpuParallaxBuilder::new()
///     .static_layer(&sky_sprite, -100)
///     .layer(&mountains_sprite, 0.3, -80)
///         .with_tile_mode(TileMode::InfiniteScroll)
///         .with_auto_scroll(10.0)
///     .layer(&dunes_sprite, 0.8, -60)
///         .with_tile_mode(TileMode::InfiniteScroll)
///         .with_auto_scroll(50.0)
///     .build();
/// ```
pub struct GpuParallaxBuilder {
  layers: Vec<GpuParallaxLayer>,
}

impl Default for GpuParallaxBuilder {
  fn default() -> Self {
    Self::new()
  }
}

impl GpuParallaxBuilder {
  /// Create a new builder
  pub fn new() -> Self {
    Self { layers: Vec::new() }
  }

  /// Add a static layer (scroll_speed = 0)
  pub fn static_layer(mut self, sprite: &SpriteHandle, z_order: i32) -> Self {
    self
      .layers
      .push(GpuParallaxLayer::new(sprite, 0.0, z_order));
    self
  }

  /// Add a layer with parallax scrolling
  pub fn layer(mut self, sprite: &SpriteHandle, scroll_speed: f32, z_order: i32) -> Self {
    self
      .layers
      .push(GpuParallaxLayer::new(sprite, scroll_speed, z_order));
    self
  }

  /// Configure the last added layer with auto-scroll
  pub fn with_auto_scroll(mut self, speed: f32) -> Self {
    if let Some(layer) = self.layers.last_mut() {
      layer.auto_scroll_speed = speed;
    }
    self
  }

  /// Configure the last added layer with a y offset
  pub fn with_y_offset(mut self, offset: f32) -> Self {
    if let Some(layer) = self.layers.last_mut() {
      layer.y_offset = offset;
    }
    self
  }

  /// Configure the last added layer with a tile mode
  pub fn with_tile_mode(mut self, mode: TileMode) -> Self {
    if let Some(layer) = self.layers.last_mut() {
      layer.tile_mode = mode;
    }
    self
  }

  /// Configure the last added layer with scale
  pub fn with_scale(mut self, scale: f32) -> Self {
    if let Some(layer) = self.layers.last_mut() {
      layer.scale = scale;
    }
    self
  }

  /// Build the GpuParallaxBackground
  pub fn build(self) -> GpuParallaxBackground {
    let mut bg = GpuParallaxBackground::new();
    for layer in self.layers {
      bg.add_layer(layer);
    }
    bg
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::core::graphics::Camera2d;

  #[test]
  fn a_layer_scrolls_with_the_camera_on_both_axes() {
    let layer = GpuParallaxLayer::test_layer(64, 64).with_scroll_speed(0.5);
    let near = CameraView::compute(
      Vec2::new(100.0, 100.0),
      0.0,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(64.0, 64.0),
    );
    let far = CameraView::compute(
      Vec2::new(120.0, 140.0),
      0.0,
      &Camera2d::default(),
      Vec2::ZERO,
      Vec2::new(64.0, 64.0),
    );
    let a = layer.world_instances(&near, 0.0)[0].transform[2];
    let b = layer.world_instances(&far, 0.0)[0].transform[2];
    // Half the camera's movement in world space: 10 right and 20 down
    assert_eq!((b[0] - a[0], b[1] - a[1]), (10.0, 20.0));
  }
}
