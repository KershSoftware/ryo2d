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
use crate::core::graphics::parallax::TileMode;
use crate::core::graphics::sprite_gpu::SpriteInstance;

/// One layer of a [`GpuParallaxBackground`], drawn as instanced GPU sprites.
#[derive(Clone)]
pub struct GpuParallaxLayer {
  /// Texture ID (from SpriteHandle.id())
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
  sprite_width: u32,
  sprite_height: u32,
  /// Scale factor
  scale: f32,
  /// Tint color (RGBA)
  tint: [u8; 4],
  /// Sprite handle for automatic GPU upload (None when created via from_id)
  sprite_handle: Option<SpriteHandle>,
}

impl GpuParallaxLayer {
  /// Create a new GPU parallax layer from a sprite handle.
  ///
  /// The sprite handle is stored internally for automatic GPU texture upload
  /// during rendering — no need to manually call `upload_sprite()`.
  pub fn new(sprite: &SpriteHandle, scroll_speed: f32, z_order: i32) -> Self {
    sprite.with_sprite(|s| Self {
      texture_id: sprite.id(),
      scroll_speed,
      auto_scroll_speed: 0.0,
      y_offset: 0.0,
      z_order,
      tile_mode: TileMode::None,
      sprite_width: s.width(),
      sprite_height: s.height(),
      scale: 1.0,
      tint: [255, 255, 255, 255],
      sprite_handle: Some(sprite.clone()),
    })
  }

  /// Create from explicit texture ID and dimensions.
  ///
  /// When using this constructor, you must manually upload the texture
  /// via `ctx.upload_sprite()` before rendering.
  pub fn from_id(
    texture_id: impl Into<String>,
    width: u32,
    height: u32,
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
      sprite_width: width,
      sprite_height: height,
      scale: 1.0,
      tint: [255, 255, 255, 255],
      sprite_handle: None,
    }
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

  /// Get the texture ID
  pub fn texture_id(&self) -> &str {
    &self.texture_id
  }

  /// Compute sprite instances for this layer
  fn compute_instances(
    &self,
    screen_width: u32,
    _screen_height: u32,
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
        let x_pos = -total_offset;
        vec![self.create_instance(x_pos, y_pos, scaled_width, scaled_height)]
      }

      TileMode::Horizontal => {
        let tiles_needed = (screen_width as f32 / scaled_width).ceil() as i32 + 1;
        let mut instances = Vec::with_capacity(tiles_needed as usize);

        for i in 0..tiles_needed {
          let x_pos = (i as f32 * scaled_width) - total_offset;
          if x_pos + scaled_width >= 0.0 && x_pos < screen_width as f32 {
            instances.push(self.create_instance(x_pos, y_pos, scaled_width, scaled_height));
          }
        }

        instances
      }

      TileMode::InfiniteScroll => {
        let tiles_needed = (screen_width as f32 / scaled_width).ceil() as i32 + 2;
        let mut instances = Vec::with_capacity(tiles_needed as usize);

        let base_offset = total_offset.rem_euclid(scaled_width);

        for i in 0..tiles_needed {
          let screen_x = (i as f32 * scaled_width) - base_offset - scaled_width;
          if screen_x + scaled_width >= 0.0 && screen_x < screen_width as f32 {
            instances.push(self.create_instance(screen_x, y_pos, scaled_width, scaled_height));
          }
        }

        instances
      }
    }
  }

  fn create_instance(&self, x: f32, y: f32, width: f32, height: f32) -> SpriteInstance {
    use crate::core::ui::Color;

    SpriteInstance::new()
      .with_transform(x + width / 2.0, y + height / 2.0, 0.0, width, height)
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

  /// Remove a layer by texture ID
  pub fn remove_layer(&mut self, texture_id: &str) -> Option<GpuParallaxLayer> {
    if let Some(pos) = self.layers.iter().position(|l| l.texture_id == texture_id) {
      Some(self.layers.remove(pos))
    } else {
      None
    }
  }

  /// Get the number of layers
  pub fn layer_count(&self) -> usize {
    self.layers.len()
  }

  /// Get a mutable reference to a layer by texture ID
  pub fn get_layer_mut(&mut self, texture_id: &str) -> Option<&mut GpuParallaxLayer> {
    self.layers.iter_mut().find(|l| l.texture_id == texture_id)
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
  /// Layers created with [`GpuParallaxLayer::new()`] have their textures
  /// uploaded automatically. Layers created with [`GpuParallaxLayer::from_id()`]
  /// require manual upload via `ctx.upload_sprite()` before this call.
  pub fn render(&self, ctx: &mut GpuRenderContext) {
    let screen_width = ctx.screen_width();
    let screen_height = ctx.screen_height();
    let camera_x = ctx.camera().x;

    for layer in &self.layers {
      // Auto-upload texture if sprite handle is available
      if let Some(ref handle) = layer.sprite_handle {
        ctx.upload_sprite(handle);
      } else if !ctx.has_texture(&layer.texture_id) {
        continue;
      }

      let instances =
        layer.compute_instances(screen_width, screen_height, camera_x, self.elapsed_time);

      for instance in instances {
        ctx.add_sprite_instance(&layer.texture_id, instance, layer.z_order, false);
      }
    }
  }

  /// Upload additional textures to the GPU.
  ///
  /// This is only needed for layers created with [`GpuParallaxLayer::from_id()`].
  /// Layers created with [`GpuParallaxLayer::new()`] upload automatically during render.
  pub fn upload_textures(&self, ctx: &mut GpuRenderContext, sprites: &[&SpriteHandle]) {
    for sprite in sprites {
      ctx.upload_sprite(sprite);
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
