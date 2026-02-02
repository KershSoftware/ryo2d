//! Parallax background system for layered scrolling backgrounds.
//!
//! This module provides a system-based approach to parallax backgrounds,
//! where multiple sprite layers scroll at different speeds relative to
//! the camera, creating depth and movement.
//!
//! # Example
//!
//! ```ignore
//! // In scene/game struct:
//! struct DesertScene {
//!     background: ParallaxBackground,
//! }
//!
//! // In init:
//! let (screen_w, screen_h) = ctx.screen_size();
//! self.background = ParallaxBackground::builder(screen_w as u32, screen_h as u32)
//!     .static_layer(sky_sprite, -100)              // Doesn't move
//!     .layer(mountains_sprite, 0.2, -80)           // 20% camera speed
//!     .layer(dunes_mid_sprite, 0.5, -60)           // 50% camera speed
//!     .layer(dunes_front_sprite, 0.8, -40)         // 80% camera speed
//!     .build();
//!
//! // In render_background callback:
//! fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
//!     self.background.render(frame, width, height, camera);
//! }
//! ```

use super::Camera;
use crate::core::asset::SpriteHandle;

/// Controls how a parallax layer tiles horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TileMode {
  /// No tiling - the sprite is drawn once at its position.
  /// Suitable for static sky backgrounds or non-repeating elements.
  #[default]
  None,

  /// Tile horizontally to fill the screen width.
  /// The sprite repeats seamlessly across the screen.
  Horizontal,

  /// Infinite horizontal scrolling with seamless wrapping.
  /// The sprite tiles continuously as the camera moves,
  /// creating an endless scrolling effect.
  InfiniteScroll,
}

/// A single layer in a parallax background.
///
/// Each layer has a sprite, scroll speed, z-order, and optional y-offset.
/// Layers with lower z-order are drawn first (behind layers with higher z-order).
#[derive(Debug, Clone)]
pub struct ParallaxLayer {
  /// The sprite to render for this layer
  pub sprite: SpriteHandle,

  /// Scroll speed relative to camera movement.
  /// - 0.0 = static (doesn't move with camera)
  /// - 0.5 = moves at 50% camera speed (appears far away)
  /// - 1.0 = moves with camera (appears at camera distance)
  /// - Values > 1.0 create foreground parallax (rarely used)
  pub scroll_speed: f32,

  /// Auto-scroll speed in pixels per second (independent of camera).
  /// Use this for fixed-camera games where you want continuous scrolling.
  /// Positive values scroll left (layer moves right-to-left).
  pub auto_scroll_speed: f32,

  /// Z-order for layer sorting. Lower values are drawn first (behind).
  pub z_order: i32,

  /// Vertical offset from the top of the screen in pixels.
  /// Use negative values to position layers above the default position.
  pub y_offset: f32,

  /// How this layer tiles horizontally.
  pub tile_mode: TileMode,
}

impl ParallaxLayer {
  /// Create a new parallax layer with default settings.
  pub fn new(sprite: SpriteHandle, scroll_speed: f32, z_order: i32) -> Self {
    Self {
      sprite,
      scroll_speed,
      auto_scroll_speed: 0.0,
      z_order,
      y_offset: 0.0,
      tile_mode: TileMode::InfiniteScroll,
    }
  }

  /// Create a static layer that doesn't scroll.
  pub fn static_layer(sprite: SpriteHandle, z_order: i32) -> Self {
    Self {
      sprite,
      scroll_speed: 0.0,
      auto_scroll_speed: 0.0,
      z_order,
      y_offset: 0.0,
      tile_mode: TileMode::None,
    }
  }

  /// Set the vertical offset for this layer.
  pub fn with_y_offset(mut self, offset: f32) -> Self {
    self.y_offset = offset;
    self
  }

  /// Set the tile mode for this layer.
  pub fn with_tile_mode(mut self, mode: TileMode) -> Self {
    self.tile_mode = mode;
    self
  }

  /// Set the auto-scroll speed (pixels per second).
  /// Use this for fixed-camera games.
  pub fn with_auto_scroll(mut self, speed: f32) -> Self {
    self.auto_scroll_speed = speed;
    self
  }
}

/// A parallax background composed of multiple scrolling layers.
///
/// The background manages multiple layers that scroll at different speeds
/// relative to the camera, creating a sense of depth.
#[derive(Debug, Clone)]
pub struct ParallaxBackground {
  /// The layers in this background, sorted by z-order.
  layers: Vec<ParallaxLayer>,

  /// Screen width for tiling calculations.
  screen_width: u32,

  /// Screen height for positioning.
  screen_height: u32,

  /// Elapsed time for auto-scroll calculations.
  elapsed_time: f32,

  /// Whether to scale layers to fill screen height.
  scale_to_fit: bool,
}

impl Default for ParallaxBackground {
  fn default() -> Self {
    Self::new(800, 600)
  }
}

impl ParallaxBackground {
  /// Create a new empty parallax background.
  pub fn new(screen_width: u32, screen_height: u32) -> Self {
    Self {
      layers: Vec::new(),
      screen_width,
      screen_height,
      elapsed_time: 0.0,
      scale_to_fit: false,
    }
  }

  /// Create a builder for fluent parallax background construction.
  pub fn builder(screen_width: u32, screen_height: u32) -> ParallaxBackgroundBuilder {
    ParallaxBackgroundBuilder::new(screen_width, screen_height)
  }

  /// Add a layer to the background.
  ///
  /// Layers are automatically sorted by z-order when rendered.
  pub fn add_layer(&mut self, layer: ParallaxLayer) {
    self.layers.push(layer);
    self.layers.sort_by_key(|l| l.z_order);
  }

  /// Update screen dimensions (call when resolution changes).
  pub fn set_screen_size(&mut self, width: u32, height: u32) {
    self.screen_width = width;
    self.screen_height = height;
  }

  /// Enable or disable scale-to-fit mode.
  /// When enabled, layers are scaled to fill the screen height.
  pub fn set_scale_to_fit(&mut self, enabled: bool) {
    self.scale_to_fit = enabled;
  }

  /// Get the number of layers in this background.
  pub fn layer_count(&self) -> usize {
    self.layers.len()
  }

  /// Update the parallax background (call each frame for auto-scroll).
  ///
  /// # Arguments
  /// * `dt` - Delta time in seconds
  pub fn update(&mut self, dt: f32) {
    self.elapsed_time += dt;
  }

  /// Render all layers to the frame buffer.
  ///
  /// This should be called from `render_background` in your Game/Scene implementation.
  ///
  /// # Arguments
  /// * `frame` - The pixel buffer (RGBA, 4 bytes per pixel)
  /// * `frame_width` - Width of the frame buffer
  /// * `frame_height` - Height of the frame buffer
  /// * `camera` - The current camera (used for parallax offset calculation)
  pub fn render(&self, frame: &mut [u8], frame_width: u32, frame_height: u32, camera: &Camera) {
    for layer in &self.layers {
      self.render_layer(frame, frame_width, frame_height, camera, layer);
    }
  }

  /// Render a single layer to the frame buffer.
  fn render_layer(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    camera: &Camera,
    layer: &ParallaxLayer,
  ) {
    let sprite_width = layer.sprite.width() as f32;
    let sprite_height = layer.sprite.height() as f32;

    if sprite_width <= 0.0 || sprite_height <= 0.0 {
      return;
    }

    // Calculate scale factor if scale_to_fit is enabled
    let scale = if self.scale_to_fit {
      frame_height as f32 / sprite_height
    } else {
      1.0
    };

    let scaled_width = sprite_width * scale;
    let _scaled_height = sprite_height * scale;

    // Calculate the y position for this layer (scaled offset)
    let y_pos = (layer.y_offset * scale) as i32;

    // Calculate total offset: camera-based parallax + time-based auto-scroll
    let camera_offset = camera.x * layer.scroll_speed;
    let auto_offset = self.elapsed_time * layer.auto_scroll_speed;
    let total_offset = (camera_offset + auto_offset) * scale;

    match layer.tile_mode {
      TileMode::None => {
        // Draw the sprite once, offset by parallax
        let x_pos = -total_offset as i32;
        layer.sprite.with_sprite(|sprite| {
          if scale != 1.0 {
            sprite.draw_scaled(frame, frame_width, frame_height, x_pos, y_pos, scale);
          } else {
            sprite.draw(frame, frame_width, frame_height, x_pos, y_pos);
          }
        });
      }

      TileMode::Horizontal => {
        // Tile horizontally to fill the screen, with parallax offset
        let tiles_needed = (frame_width as f32 / scaled_width).ceil() as i32 + 1;

        for i in 0..tiles_needed {
          let x_pos = (i as f32 * scaled_width - total_offset) as i32;
          // Only draw if the tile is at least partially visible
          if x_pos + scaled_width as i32 >= 0 && x_pos < frame_width as i32 {
            layer.sprite.with_sprite(|sprite| {
              if scale != 1.0 {
                sprite.draw_scaled(frame, frame_width, frame_height, x_pos, y_pos, scale);
              } else {
                sprite.draw(frame, frame_width, frame_height, x_pos, y_pos);
              }
            });
          }
        }
      }

      TileMode::InfiniteScroll => {
        // Infinite horizontal scrolling with seamless wrapping
        // Calculate how many tiles we need to cover the screen plus buffer
        let tiles_needed = (frame_width as f32 / scaled_width).ceil() as i32 + 2;

        // Calculate base offset with modulo for seamless wrapping
        let base_offset = total_offset.rem_euclid(scaled_width);

        for i in 0..tiles_needed {
          // Calculate screen x position for this tile
          // Start one tile to the left (-scaled_width) to ensure seamless scrolling
          let screen_x = (i as f32 * scaled_width) - base_offset - scaled_width;
          let x_pos = screen_x as i32;

          // Only draw if the tile is at least partially visible
          if x_pos + scaled_width as i32 >= 0 && x_pos < frame_width as i32 {
            layer.sprite.with_sprite(|sprite| {
              if scale != 1.0 {
                sprite.draw_scaled(frame, frame_width, frame_height, x_pos, y_pos, scale);
              } else {
                sprite.draw(frame, frame_width, frame_height, x_pos, y_pos);
              }
            });
          }
        }
      }
    }
  }
}

/// Builder for constructing parallax backgrounds with a fluent API.
///
/// # Example
///
/// ```ignore
/// let background = ParallaxBackground::builder(800, 600)
///     .static_layer(sky_sprite, -100)
///     .layer(mountains_sprite, 0.2, -80)
///     .layer(dunes_sprite, 0.5, -60)
///     .build();
/// ```
#[derive(Debug)]
pub struct ParallaxBackgroundBuilder {
  background: ParallaxBackground,
}

impl ParallaxBackgroundBuilder {
  /// Create a new builder with the given screen dimensions.
  pub fn new(screen_width: u32, screen_height: u32) -> Self {
    Self {
      background: ParallaxBackground::new(screen_width, screen_height),
    }
  }

  /// Add a scrolling layer with infinite horizontal tiling.
  ///
  /// # Arguments
  /// * `sprite` - The sprite to use for this layer
  /// * `scroll_speed` - Speed relative to camera (0.0 = static, 1.0 = camera speed)
  /// * `z_order` - Drawing order (lower = drawn first/behind)
  pub fn layer(mut self, sprite: SpriteHandle, scroll_speed: f32, z_order: i32) -> Self {
    let layer = ParallaxLayer::new(sprite, scroll_speed, z_order);
    self.background.add_layer(layer);
    self
  }

  /// Add a scrolling layer with custom y-offset and infinite horizontal tiling.
  ///
  /// # Arguments
  /// * `sprite` - The sprite to use for this layer
  /// * `scroll_speed` - Speed relative to camera (0.0 = static, 1.0 = camera speed)
  /// * `z_order` - Drawing order (lower = drawn first/behind)
  /// * `y_offset` - Vertical offset in pixels from top of screen
  pub fn layer_at(
    mut self,
    sprite: SpriteHandle,
    scroll_speed: f32,
    z_order: i32,
    y_offset: f32,
  ) -> Self {
    let layer = ParallaxLayer::new(sprite, scroll_speed, z_order).with_y_offset(y_offset);
    self.background.add_layer(layer);
    self
  }

  /// Add a static layer that doesn't scroll with the camera.
  ///
  /// Static layers are typically used for far backgrounds like sky gradients
  /// that shouldn't move regardless of camera position.
  ///
  /// # Arguments
  /// * `sprite` - The sprite to use for this layer
  /// * `z_order` - Drawing order (lower = drawn first/behind)
  pub fn static_layer(mut self, sprite: SpriteHandle, z_order: i32) -> Self {
    let layer = ParallaxLayer::static_layer(sprite, z_order);
    self.background.add_layer(layer);
    self
  }

  /// Add a static layer at a specific y-offset.
  ///
  /// # Arguments
  /// * `sprite` - The sprite to use for this layer
  /// * `z_order` - Drawing order (lower = drawn first/behind)
  /// * `y_offset` - Vertical offset in pixels from top of screen
  pub fn static_layer_at(mut self, sprite: SpriteHandle, z_order: i32, y_offset: f32) -> Self {
    let layer = ParallaxLayer::static_layer(sprite, z_order).with_y_offset(y_offset);
    self.background.add_layer(layer);
    self
  }

  /// Add a layer with custom tile mode.
  ///
  /// # Arguments
  /// * `sprite` - The sprite to use for this layer
  /// * `scroll_speed` - Speed relative to camera (0.0 = static, 1.0 = camera speed)
  /// * `z_order` - Drawing order (lower = drawn first/behind)
  /// * `tile_mode` - How this layer should tile horizontally
  pub fn layer_with_mode(
    mut self,
    sprite: SpriteHandle,
    scroll_speed: f32,
    z_order: i32,
    tile_mode: TileMode,
  ) -> Self {
    let layer = ParallaxLayer::new(sprite, scroll_speed, z_order).with_tile_mode(tile_mode);
    self.background.add_layer(layer);
    self
  }

  /// Add a fully customized layer.
  pub fn custom_layer(mut self, layer: ParallaxLayer) -> Self {
    self.background.add_layer(layer);
    self
  }

  /// Add an auto-scrolling layer (for fixed-camera games).
  ///
  /// # Arguments
  /// * `sprite` - The sprite to use for this layer
  /// * `auto_scroll_speed` - Scroll speed in pixels per second
  /// * `z_order` - Drawing order (lower = drawn first/behind)
  pub fn auto_scroll_layer(
    mut self,
    sprite: SpriteHandle,
    auto_scroll_speed: f32,
    z_order: i32,
  ) -> Self {
    let layer = ParallaxLayer::new(sprite, 0.0, z_order).with_auto_scroll(auto_scroll_speed);
    self.background.add_layer(layer);
    self
  }

  /// Enable scale-to-fit mode.
  /// When enabled, all layers are scaled to fill the screen height.
  pub fn scale_to_fit(mut self) -> Self {
    self.background.scale_to_fit = true;
    self
  }

  /// Build the final ParallaxBackground.
  pub fn build(self) -> ParallaxBackground {
    self.background
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_tile_mode_default() {
    assert_eq!(TileMode::default(), TileMode::None);
  }

  #[test]
  fn test_parallax_background_default() {
    let bg = ParallaxBackground::default();
    assert_eq!(bg.layer_count(), 0);
    assert_eq!(bg.screen_width, 800);
    assert_eq!(bg.screen_height, 600);
  }

  #[test]
  fn test_builder_creates_empty_background() {
    let bg = ParallaxBackground::builder(1280, 720).build();
    assert_eq!(bg.layer_count(), 0);
    assert_eq!(bg.screen_width, 1280);
    assert_eq!(bg.screen_height, 720);
  }

  #[test]
  fn test_parallax_layer_new() {
    // Can't fully test without SpriteHandle, but we can test the structure
    let bg = ParallaxBackground::new(800, 600);
    assert_eq!(bg.screen_width, 800);
    assert_eq!(bg.screen_height, 600);
  }
}
