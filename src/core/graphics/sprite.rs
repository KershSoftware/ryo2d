use std::ffi::OsStr;
use std::path::Path;

use crate::core::error::{SpriteError, SpriteResult};
use crate::core::ui::Color;

/// A 2D sprite that can be drawn to the screen
#[derive(Debug, Clone)]
pub struct Sprite {
  /// Raw RGBA pixel data (4 bytes per pixel)
  data: Vec<u8>,
  /// Width in pixels
  width: u32,
  /// Height in pixels
  height: u32,
  /// Whether all pixels are fully opaque (alpha=255), enabling fast row-copy rendering
  is_opaque: bool,
}

impl Sprite {
  /// Check if all pixels in the data are fully opaque (alpha=255)
  fn check_opaque(data: &[u8]) -> bool {
    data.chunks_exact(4).all(|pixel| pixel[3] == 255)
  }

  /// Create a new sprite from raw RGBA data
  pub fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(
      data.len(),
      (width * height * 4) as usize,
      "Data length must match width * height * 4"
    );
    let is_opaque = Self::check_opaque(&data);
    Sprite {
      data,
      width,
      height,
      is_opaque,
    }
  }

  /// Create a solid color rectangle sprite
  pub fn solid_rect(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for pixel in data.chunks_exact_mut(4) {
      pixel[0] = r;
      pixel[1] = g;
      pixel[2] = b;
      pixel[3] = a;
    }
    Sprite {
      data,
      width,
      height,
      is_opaque: a == 255,
    }
  }

  /// Create a solid color circle sprite with anti-aliasing
  ///
  /// The sprite will be `diameter x diameter` pixels, with a circle centered in it.
  /// Note: All pixels (including transparent ones) have the same RGB color to prevent
  /// black fringing during bilinear interpolation in rotated/scaled rendering.
  pub fn solid_circle(diameter: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
    // Initialize all pixels with the target color (not black) to prevent
    // black fringing during bilinear interpolation at edges
    let mut data = vec![0u8; (diameter * diameter * 4) as usize];
    for pixel in data.chunks_exact_mut(4) {
      pixel[0] = r;
      pixel[1] = g;
      pixel[2] = b;
      pixel[3] = 0; // Alpha will be set per-pixel below
    }

    let radius = diameter as f32 / 2.0;
    let center = radius;

    for y in 0..diameter {
      for x in 0..diameter {
        let dx = x as f32 + 0.5 - center;
        let dy = y as f32 + 0.5 - center;
        let dist = (dx * dx + dy * dy).sqrt();

        // Anti-aliased edge
        let alpha = if dist <= radius - 1.0 {
          a as f32
        } else if dist <= radius + 1.0 {
          // Smooth falloff at edge
          a as f32 * (radius + 1.0 - dist)
        } else {
          0.0
        };

        let idx = ((y * diameter + x) * 4) as usize;
        data[idx + 3] = alpha as u8;
      }
    }

    // Circles with anti-aliasing are never fully opaque
    Sprite {
      data,
      width: diameter,
      height: diameter,
      is_opaque: false,
    }
  }

  /// Create a solid-colored ellipse sprite with anti-aliased edges.
  ///
  /// The sprite will be `width x height` pixels, with an ellipse filling it.
  /// Useful for blob shadows and other oval shapes.
  ///
  /// Note: All pixels (including transparent ones) have the same RGB color to prevent
  /// black fringing during bilinear interpolation in rotated/scaled rendering.
  pub fn solid_ellipse(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
    // Initialize all pixels with the target color to prevent fringing
    let mut data = vec![0u8; (width * height * 4) as usize];
    for pixel in data.chunks_exact_mut(4) {
      pixel[0] = r;
      pixel[1] = g;
      pixel[2] = b;
      pixel[3] = 0; // Alpha will be set per-pixel below
    }

    let radius_x = width as f32 / 2.0;
    let radius_y = height as f32 / 2.0;
    let center_x = radius_x;
    let center_y = radius_y;

    for y in 0..height {
      for x in 0..width {
        let dx = (x as f32 + 0.5 - center_x) / radius_x;
        let dy = (y as f32 + 0.5 - center_y) / radius_y;
        let dist = (dx * dx + dy * dy).sqrt();

        // Anti-aliased edge (dist is normalized, so 1.0 = edge)
        let edge_width = 1.0 / radius_x.min(radius_y); // Thinner AA for larger ellipses
        let alpha = if dist <= 1.0 - edge_width {
          a as f32
        } else if dist <= 1.0 + edge_width {
          // Smooth falloff at edge
          a as f32 * ((1.0 + edge_width - dist) / (2.0 * edge_width))
        } else {
          0.0
        };

        let idx = ((y * width + x) * 4) as usize;
        data[idx + 3] = alpha as u8;
      }
    }

    Sprite {
      data,
      width,
      height,
      is_opaque: false,
    }
  }

  /// Load a sprite from an image file (PNG, JPEG, SVG, etc.)
  ///
  /// SVG files are rasterized at their native size. Use `load_svg_scaled`
  /// if you need to render at a specific size.
  pub fn load<P: AsRef<Path>>(path: P) -> SpriteResult<Self> {
    let path_ref = path.as_ref();

    // Check if it's an SVG file
    if path_ref.extension().and_then(OsStr::to_str) == Some("svg") {
      return Self::load_svg(path_ref, 1.0);
    }

    // Use image crate for raster formats
    let img = image::open(path_ref).map_err(|e| SpriteError::DecodeError {
      path: path_ref.to_path_buf(),
      reason: e.to_string(),
    })?;

    let rgba = img.into_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    let data = rgba.into_raw();
    let is_opaque = Self::check_opaque(&data);

    Ok(Sprite {
      data,
      width,
      height,
      is_opaque,
    })
  }

  /// Load an SVG file and rasterize it at a specific scale factor.
  ///
  /// # Arguments
  /// * `path` - Path to the SVG file
  /// * `scale` - Scale factor (1.0 = native size, 2.0 = double size, etc.)
  ///
  /// # Example
  /// ```rust,ignore
  /// // Load SVG at native size
  /// let icon = Sprite::load_svg("icon.svg", 1.0)?;
  ///
  /// // Load SVG at 2x size for high-DPI displays
  /// let icon_hd = Sprite::load_svg("icon.svg", 2.0)?;
  /// ```
  pub fn load_svg<P: AsRef<Path>>(path: P, scale: f32) -> SpriteResult<Self> {
    let path_ref = path.as_ref();

    // Read SVG data
    let svg_data = std::fs::read(path_ref).map_err(|e| SpriteError::DecodeError {
      path: path_ref.to_path_buf(),
      reason: e.to_string(),
    })?;

    Self::from_svg_data(&svg_data, scale).map_err(|reason| SpriteError::DecodeError {
      path: path_ref.to_path_buf(),
      reason,
    })
  }

  /// Load an SVG and rasterize it to fit within specific dimensions.
  ///
  /// The aspect ratio is preserved - the SVG will be scaled to fit within
  /// the given bounds while maintaining proportions.
  ///
  /// # Arguments
  /// * `path` - Path to the SVG file
  /// * `max_width` - Maximum width in pixels
  /// * `max_height` - Maximum height in pixels
  pub fn load_svg_fit<P: AsRef<Path>>(
    path: P,
    max_width: u32,
    max_height: u32,
  ) -> SpriteResult<Self> {
    let path_ref = path.as_ref();

    let svg_data = std::fs::read(path_ref).map_err(|e| SpriteError::DecodeError {
      path: path_ref.to_path_buf(),
      reason: e.to_string(),
    })?;

    Self::from_svg_data_fit(&svg_data, max_width, max_height).map_err(|reason| {
      SpriteError::DecodeError {
        path: path_ref.to_path_buf(),
        reason,
      }
    })
  }

  /// Create a sprite from raw SVG data with a scale factor.
  pub fn from_svg_data(svg_data: &[u8], scale: f32) -> Result<Self, String> {
    use resvg::usvg;
    use tiny_skia::{Pixmap, Transform};

    // Parse SVG
    let opt = usvg::Options::default();
    let tree =
      usvg::Tree::from_data(svg_data, &opt).map_err(|e| format!("Failed to parse SVG: {}", e))?;

    // Calculate scaled dimensions
    let size = tree.size();
    let width = (size.width() * scale) as u32;
    let height = (size.height() * scale) as u32;

    if width == 0 || height == 0 {
      return Err("SVG has zero dimensions".to_string());
    }

    // Create pixmap and render
    let mut pixmap =
      Pixmap::new(width, height).ok_or_else(|| "Failed to create pixmap".to_string())?;

    let transform = Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Convert to RGBA (tiny-skia uses premultiplied alpha, need to unpremultiply)
    let data = Self::unpremultiply_alpha(pixmap.data());
    let is_opaque = Self::check_opaque(&data);

    Ok(Sprite {
      data,
      width,
      height,
      is_opaque,
    })
  }

  /// Create a sprite from raw SVG data, fitting within max dimensions.
  pub fn from_svg_data_fit(
    svg_data: &[u8],
    max_width: u32,
    max_height: u32,
  ) -> Result<Self, String> {
    use resvg::usvg;

    // Parse SVG to get dimensions
    let opt = usvg::Options::default();
    let tree =
      usvg::Tree::from_data(svg_data, &opt).map_err(|e| format!("Failed to parse SVG: {}", e))?;

    let size = tree.size();
    let svg_width = size.width();
    let svg_height = size.height();

    if svg_width == 0.0 || svg_height == 0.0 {
      return Err("SVG has zero dimensions".to_string());
    }

    // Calculate scale to fit within bounds
    let scale_x = max_width as f32 / svg_width;
    let scale_y = max_height as f32 / svg_height;
    let scale = scale_x.min(scale_y);

    Self::from_svg_data(svg_data, scale)
  }

  /// Convert premultiplied alpha to straight alpha
  fn unpremultiply_alpha(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(4) {
      let a = chunk[3] as f32 / 255.0;
      if a > 0.0 {
        result.push((chunk[0] as f32 / a).min(255.0) as u8);
        result.push((chunk[1] as f32 / a).min(255.0) as u8);
        result.push((chunk[2] as f32 / a).min(255.0) as u8);
        result.push(chunk[3]);
      } else {
        result.extend_from_slice(&[0, 0, 0, 0]);
      }
    }
    result
  }

  /// Get the sprite width
  pub fn width(&self) -> u32 {
    self.width
  }

  /// Get the sprite height
  pub fn height(&self) -> u32 {
    self.height
  }

  /// Check if the sprite is fully opaque (no transparent or semi-transparent pixels)
  pub fn is_opaque(&self) -> bool {
    self.is_opaque
  }

  /// Get the raw RGBA pixel data
  ///
  /// Returns a slice of the raw pixel data. Each pixel is 4 bytes: R, G, B, A.
  /// The data is stored in row-major order (left to right, top to bottom).
  pub fn data(&self) -> &[u8] {
    &self.data
  }

  /// Create a new sprite with pre-scaled pixel data (nearest-neighbor).
  ///
  /// This is useful for caching scaled versions of sprites to avoid
  /// expensive per-frame scaling operations.
  ///
  /// # Arguments
  /// * `scale` - Uniform scale factor (1.0 = original size)
  pub fn scaled(&self, scale: f32) -> Self {
    self.scaled_xy(scale, scale)
  }

  /// Create a new sprite with pre-scaled pixel data (nearest-neighbor, non-uniform).
  ///
  /// # Arguments
  /// * `scale_x` - Horizontal scale factor
  /// * `scale_y` - Vertical scale factor
  pub fn scaled_xy(&self, scale_x: f32, scale_y: f32) -> Self {
    if scale_x <= 0.0 || scale_y <= 0.0 {
      return Self::from_rgba(vec![], 0, 0);
    }

    let new_width = (self.width as f32 * scale_x).ceil() as u32;
    let new_height = (self.height as f32 * scale_y).ceil() as u32;

    if new_width == 0 || new_height == 0 {
      return Self::from_rgba(vec![], 0, 0);
    }

    let mut data = vec![0u8; (new_width * new_height * 4) as usize];

    for dy in 0..new_height {
      for dx in 0..new_width {
        // Map destination pixel back to source (nearest neighbor)
        let sx = ((dx as f32) / scale_x) as u32;
        let sy = ((dy as f32) / scale_y) as u32;

        // Clamp to sprite bounds
        let sx = sx.min(self.width - 1);
        let sy = sy.min(self.height - 1);

        let src_idx = ((sy * self.width + sx) * 4) as usize;
        let dst_idx = ((dy * new_width + dx) * 4) as usize;

        data[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
      }
    }

    // Scaled sprite inherits opacity from source (scaling doesn't change alpha)
    Sprite {
      data,
      width: new_width,
      height: new_height,
      is_opaque: self.is_opaque,
    }
  }

  /// Draw the sprite to a pixel buffer at the given position
  ///
  /// # Arguments
  /// * `frame` - The pixel buffer (RGBA, 4 bytes per pixel)
  /// * `frame_width` - Width of the frame buffer
  /// * `frame_height` - Height of the frame buffer
  /// * `x` - X position to draw at (can be negative for partial off-screen)
  /// * `y` - Y position to draw at (can be negative for partial off-screen)
  pub fn draw(&self, frame: &mut [u8], frame_width: u32, frame_height: u32, x: i32, y: i32) {
    let frame_w = frame_width as i32;
    let frame_h = frame_height as i32;
    let sprite_w = self.width as i32;
    let sprite_h = self.height as i32;

    // Calculate visible region (clip to screen bounds)
    let start_sx = (-x).max(0);
    let start_sy = (-y).max(0);
    let end_sx = (frame_w - x).min(sprite_w);
    let end_sy = (frame_h - y).min(sprite_h);

    // Early exit if completely off-screen
    if start_sx >= end_sx || start_sy >= end_sy {
      return;
    }

    // Fast path for fully opaque sprites: use row-based memcpy
    if self.is_opaque {
      let row_width = (end_sx - start_sx) as usize * 4;
      for sy in start_sy..end_sy {
        let fy = (y + sy) as usize;
        let fx = (x + start_sx) as usize;

        let src_row_start = ((sy as u32 * self.width + start_sx as u32) * 4) as usize;
        let dst_row_start = (fy * frame_width as usize + fx) * 4;

        frame[dst_row_start..dst_row_start + row_width]
          .copy_from_slice(&self.data[src_row_start..src_row_start + row_width]);
      }
      return;
    }

    // Slow path for sprites with transparency: per-pixel alpha blending
    for sy in start_sy..end_sy {
      for sx in start_sx..end_sx {
        let fx = (x + sx) as usize;
        let fy = (y + sy) as usize;

        let src_idx = ((sy as u32 * self.width + sx as u32) * 4) as usize;
        let dst_idx = (fy * frame_width as usize + fx) * 4;

        let src_a = self.data[src_idx + 3];

        if src_a == 255 {
          // Fully opaque - just copy
          frame[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
        } else if src_a > 0 {
          // Alpha blending
          let src_r = self.data[src_idx] as u16;
          let src_g = self.data[src_idx + 1] as u16;
          let src_b = self.data[src_idx + 2] as u16;
          let alpha = src_a as u16;
          let inv_alpha = 255 - alpha;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = ((src_r * alpha + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = ((src_g * alpha + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = ((src_b * alpha + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
        // src_a == 0: fully transparent, skip
      }
    }
  }

  /// Draw the sprite scaled (nearest-neighbor) to a pixel buffer
  ///
  /// Uses nearest-neighbor sampling for crisp pixel-art scaling.
  pub fn draw_scaled(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    scale: f32,
  ) {
    self.draw_scaled_xy(frame, frame_width, frame_height, x, y, scale, scale);
  }

  /// Draw the sprite with non-uniform scaling (nearest-neighbor)
  ///
  /// Uses nearest-neighbor sampling for crisp pixel-art scaling.
  /// Allows different scale factors for x and y axes.
  #[allow(clippy::too_many_arguments)]
  pub fn draw_scaled_xy(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    scale_x: f32,
    scale_y: f32,
  ) {
    if scale_x <= 0.0 || scale_y <= 0.0 {
      return;
    }

    let frame_w = frame_width as i32;
    let frame_h = frame_height as i32;
    // Use ceiling to ensure we sample the rightmost/bottommost source pixels
    let scaled_w = (self.width as f32 * scale_x).ceil() as i32;
    let scaled_h = (self.height as f32 * scale_y).ceil() as i32;

    // Calculate visible region in screen space (clipped)
    let start_dx = (-x).max(0);
    let start_dy = (-y).max(0);
    let end_dx = (frame_w - x).min(scaled_w);
    let end_dy = (frame_h - y).min(scaled_h);

    if start_dx >= end_dx || start_dy >= end_dy {
      return;
    }

    for dy in start_dy..end_dy {
      for dx in start_dx..end_dx {
        // Map screen pixel back to source sprite (nearest neighbor)
        let sx = ((dx as f32) / scale_x) as u32;
        let sy = ((dy as f32) / scale_y) as u32;

        // Clamp to sprite bounds (safety)
        let sx = sx.min(self.width - 1);
        let sy = sy.min(self.height - 1);

        let fx = (x + dx) as usize;
        let fy = (y + dy) as usize;

        let src_idx = ((sy * self.width + sx) * 4) as usize;
        let dst_idx = (fy * frame_width as usize + fx) * 4;

        let src_a = self.data[src_idx + 3];

        if src_a == 255 {
          frame[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
        } else if src_a > 0 {
          let src_r = self.data[src_idx] as u16;
          let src_g = self.data[src_idx + 1] as u16;
          let src_b = self.data[src_idx + 2] as u16;
          let alpha = src_a as u16;
          let inv_alpha = 255 - alpha;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = ((src_r * alpha + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = ((src_g * alpha + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = ((src_b * alpha + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
      }
    }
  }

  /// Draw the sprite with scaling and optional flipping (nearest-neighbor)
  ///
  /// Uses nearest-neighbor sampling for crisp pixel-art scaling.
  /// Supports horizontal and/or vertical flipping.
  #[allow(clippy::too_many_arguments)]
  pub fn draw_scaled_xy_flipped(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    scale_x: f32,
    scale_y: f32,
    flip_x: bool,
    flip_y: bool,
  ) {
    // If no flipping, use the regular method
    if !flip_x && !flip_y {
      return self.draw_scaled_xy(frame, frame_width, frame_height, x, y, scale_x, scale_y);
    }

    if scale_x <= 0.0 || scale_y <= 0.0 {
      return;
    }

    let frame_w = frame_width as i32;
    let frame_h = frame_height as i32;
    // Use ceiling to ensure we sample the rightmost/bottommost source pixels
    let scaled_w = (self.width as f32 * scale_x).ceil() as i32;
    let scaled_h = (self.height as f32 * scale_y).ceil() as i32;

    // Calculate visible region in screen space (clipped)
    let start_dx = (-x).max(0);
    let start_dy = (-y).max(0);
    let end_dx = (frame_w - x).min(scaled_w);
    let end_dy = (frame_h - y).min(scaled_h);

    if start_dx >= end_dx || start_dy >= end_dy {
      return;
    }

    for dy in start_dy..end_dy {
      for dx in start_dx..end_dx {
        // Map screen pixel back to source sprite (nearest neighbor)
        let mut sx = ((dx as f32) / scale_x) as u32;
        let mut sy = ((dy as f32) / scale_y) as u32;

        // Apply flipping
        if flip_x {
          sx = self.width.saturating_sub(1).saturating_sub(sx);
        }
        if flip_y {
          sy = self.height.saturating_sub(1).saturating_sub(sy);
        }

        // Clamp to sprite bounds (safety)
        let sx = sx.min(self.width - 1);
        let sy = sy.min(self.height - 1);

        let fx = (x + dx) as usize;
        let fy = (y + dy) as usize;

        let src_idx = ((sy * self.width + sx) * 4) as usize;
        let dst_idx = (fy * frame_width as usize + fx) * 4;

        let src_a = self.data[src_idx + 3];

        if src_a == 255 {
          frame[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
        } else if src_a > 0 {
          let src_r = self.data[src_idx] as u16;
          let src_g = self.data[src_idx + 1] as u16;
          let src_b = self.data[src_idx + 2] as u16;
          let alpha = src_a as u16;
          let inv_alpha = 255 - alpha;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = ((src_r * alpha + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = ((src_g * alpha + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = ((src_b * alpha + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
      }
    }
  }

  /// Draw the sprite with rotation, scaling, and optional flipping
  ///
  /// Rotates around the center of the sprite.
  /// Uses nearest-neighbor sampling for pixel-art rendering.
  ///
  /// # Arguments
  ///
  /// * `rotation_degrees` - Rotation angle in degrees (positive = clockwise)
  #[allow(clippy::too_many_arguments)]
  pub fn draw_rotated(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    scale_x: f32,
    scale_y: f32,
    rotation_degrees: f32,
    flip_x: bool,
    flip_y: bool,
  ) {
    // Early exit for zero rotation - use faster path
    if rotation_degrees.abs() < 0.01 {
      return self.draw_scaled_xy_flipped(
        frame,
        frame_width,
        frame_height,
        x,
        y,
        scale_x,
        scale_y,
        flip_x,
        flip_y,
      );
    }

    if scale_x <= 0.0 || scale_y <= 0.0 {
      return;
    }

    // Use ceiling for consistent edge handling
    let scaled_w = (self.width as f32 * scale_x).ceil() as i32;
    let scaled_h = (self.height as f32 * scale_y).ceil() as i32;

    // Rotation center (center of scaled sprite in screen space)
    let center_x = x + scaled_w / 2;
    let center_y = y + scaled_h / 2;

    // Convert to radians
    let angle = rotation_degrees.to_radians();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    // Calculate bounding box of rotated sprite
    let half_w = scaled_w as f32 / 2.0;
    let half_h = scaled_h as f32 / 2.0;
    let corners = [
      (-half_w, -half_h),
      (half_w, -half_h),
      (-half_w, half_h),
      (half_w, half_h),
    ];

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for (cx, cy) in corners {
      let rx = cx * cos_a - cy * sin_a;
      let ry = cx * sin_a + cy * cos_a;
      min_x = min_x.min(rx);
      max_x = max_x.max(rx);
      min_y = min_y.min(ry);
      max_y = max_y.max(ry);
    }

    let bbox_x1 = (center_x as f32 + min_x).floor() as i32;
    let bbox_y1 = (center_y as f32 + min_y).floor() as i32;
    let bbox_x2 = (center_x as f32 + max_x).ceil() as i32;
    let bbox_y2 = (center_y as f32 + max_y).ceil() as i32;

    // Clip to frame bounds
    let start_x = bbox_x1.max(0);
    let start_y = bbox_y1.max(0);
    let end_x = bbox_x2.min(frame_width as i32);
    let end_y = bbox_y2.min(frame_height as i32);

    if start_x >= end_x || start_y >= end_y {
      return;
    }

    // Inverse rotation matrix for reverse mapping
    let inv_cos = cos_a;
    let inv_sin = -sin_a;

    for py in start_y..end_y {
      for px in start_x..end_x {
        // Sample at pixel center for better quality (0.5 offset)
        let dx = (px - center_x) as f32 + 0.5;
        let dy = (py - center_y) as f32 + 0.5;

        // Rotate backwards (inverse transform)
        let src_x = dx * inv_cos - dy * inv_sin;
        let src_y = dx * inv_sin + dy * inv_cos;

        // Map back to sprite coordinates (accounting for scale)
        let sprite_x_f = src_x / scale_x + (self.width as f32 / 2.0);
        let sprite_y_f = src_y / scale_y + (self.height as f32 / 2.0);

        // Check bounds (with margin for bilinear sampling)
        if sprite_x_f < 0.0
          || sprite_x_f >= (self.width - 1) as f32
          || sprite_y_f < 0.0
          || sprite_y_f >= (self.height - 1) as f32
        {
          continue;
        }

        // Bilinear interpolation for smoother rotation
        let x0 = sprite_x_f.floor() as u32;
        let y0 = sprite_y_f.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);

        // Interpolation weights
        let fx = sprite_x_f - x0 as f32;
        let fy = sprite_y_f - y0 as f32;
        let fx_inv = 1.0 - fx;
        let fy_inv = 1.0 - fy;

        // Apply flipping to all four sample points
        let (sx0, sx1) = if flip_x {
          ((self.width - 1) - x1, (self.width - 1) - x0)
        } else {
          (x0, x1)
        };
        let (sy0, sy1) = if flip_y {
          ((self.height - 1) - y1, (self.height - 1) - y0)
        } else {
          (y0, y1)
        };

        // Sample four pixels
        let idx00 = ((sy0 * self.width + sx0) * 4) as usize;
        let idx10 = ((sy0 * self.width + sx1) * 4) as usize;
        let idx01 = ((sy1 * self.width + sx0) * 4) as usize;
        let idx11 = ((sy1 * self.width + sx1) * 4) as usize;

        // Premultiplied alpha bilinear interpolation
        // This prevents black fringing at edges by weighting RGB by alpha
        let w00 = fx_inv * fy_inv;
        let w10 = fx * fy_inv;
        let w01 = fx_inv * fy;
        let w11 = fx * fy;

        // Get alpha values for each sample (0.0 - 1.0)
        let a00 = self.data[idx00 + 3] as f32 / 255.0;
        let a10 = self.data[idx10 + 3] as f32 / 255.0;
        let a01 = self.data[idx01 + 3] as f32 / 255.0;
        let a11 = self.data[idx11 + 3] as f32 / 255.0;

        // Interpolate premultiplied RGB (RGB * alpha)
        let mut pr = 0.0;
        let mut pg = 0.0;
        let mut pb = 0.0;
        let mut a = 0.0;

        pr += self.data[idx00] as f32 * a00 * w00;
        pg += self.data[idx00 + 1] as f32 * a00 * w00;
        pb += self.data[idx00 + 2] as f32 * a00 * w00;
        a += a00 * w00;

        pr += self.data[idx10] as f32 * a10 * w10;
        pg += self.data[idx10 + 1] as f32 * a10 * w10;
        pb += self.data[idx10 + 2] as f32 * a10 * w10;
        a += a10 * w10;

        pr += self.data[idx01] as f32 * a01 * w01;
        pg += self.data[idx01 + 1] as f32 * a01 * w01;
        pb += self.data[idx01 + 2] as f32 * a01 * w01;
        a += a01 * w01;

        pr += self.data[idx11] as f32 * a11 * w11;
        pg += self.data[idx11 + 1] as f32 * a11 * w11;
        pb += self.data[idx11 + 2] as f32 * a11 * w11;
        a += a11 * w11;

        // Skip pixels with very low alpha to prevent edge artifacts
        if a < 0.1 {
          continue;
        }

        // Unpremultiply to get final RGB
        let r = (pr / a).min(255.0);
        let g = (pg / a).min(255.0);
        let b = (pb / a).min(255.0);

        let src_a = (a * 255.0).round() as u8;
        let dst_idx = (py as usize * frame_width as usize + px as usize) * 4;

        if src_a == 255 {
          frame[dst_idx] = r.round() as u8;
          frame[dst_idx + 1] = g.round() as u8;
          frame[dst_idx + 2] = b.round() as u8;
          frame[dst_idx + 3] = 255;
        } else {
          let alpha = src_a as u16;
          let inv_alpha = 255 - alpha;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = (((r as u16) * alpha + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = (((g as u16) * alpha + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = (((b as u16) * alpha + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
      }
    }
  }

  /// Draw the sprite with scaling, flipping, and tint/alpha modulation
  ///
  /// This is the tinted version of `draw_scaled_xy_flipped` that supports
  /// runtime alpha and color modulation for tweening effects.
  ///
  /// # Arguments
  ///
  /// * `alpha` - Alpha multiplier (0.0 = invisible, 1.0 = fully opaque)
  /// * `tint` - Color multiplier applied to RGB channels
  #[allow(clippy::too_many_arguments)]
  pub fn draw_scaled_xy_flipped_tinted(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    scale_x: f32,
    scale_y: f32,
    flip_x: bool,
    flip_y: bool,
    alpha: f32,
    tint: Color,
  ) {
    // Early exit for fully transparent
    if alpha <= 0.0 {
      return;
    }

    if scale_x <= 0.0 || scale_y <= 0.0 {
      return;
    }

    let frame_w = frame_width as i32;
    let frame_h = frame_height as i32;
    let scaled_w = (self.width as f32 * scale_x).ceil() as i32;
    let scaled_h = (self.height as f32 * scale_y).ceil() as i32;

    // Calculate visible region in screen space (clipped)
    let start_dx = (-x).max(0);
    let start_dy = (-y).max(0);
    let end_dx = (frame_w - x).min(scaled_w);
    let end_dy = (frame_h - y).min(scaled_h);

    if start_dx >= end_dx || start_dy >= end_dy {
      return;
    }

    // Pre-calculate tint multipliers (0.0 - 1.0)
    let tint_r = tint.r as f32 / 255.0;
    let tint_g = tint.g as f32 / 255.0;
    let tint_b = tint.b as f32 / 255.0;
    let tint_a = (tint.a as f32 / 255.0) * alpha;

    for dy in start_dy..end_dy {
      for dx in start_dx..end_dx {
        // Map screen pixel back to source sprite (nearest neighbor)
        let mut sx = ((dx as f32) / scale_x) as u32;
        let mut sy = ((dy as f32) / scale_y) as u32;

        // Apply flipping
        if flip_x {
          sx = self.width.saturating_sub(1).saturating_sub(sx);
        }
        if flip_y {
          sy = self.height.saturating_sub(1).saturating_sub(sy);
        }

        // Clamp to sprite bounds (safety)
        let sx = sx.min(self.width - 1);
        let sy = sy.min(self.height - 1);

        let fx = (x + dx) as usize;
        let fy = (y + dy) as usize;

        let src_idx = ((sy * self.width + sx) * 4) as usize;
        let dst_idx = (fy * frame_width as usize + fx) * 4;

        let src_r = self.data[src_idx] as f32;
        let src_g = self.data[src_idx + 1] as f32;
        let src_b = self.data[src_idx + 2] as f32;
        let src_a = self.data[src_idx + 3] as f32 / 255.0;

        // Apply tint to source color
        let tinted_r = (src_r * tint_r).min(255.0);
        let tinted_g = (src_g * tint_g).min(255.0);
        let tinted_b = (src_b * tint_b).min(255.0);

        // Combined alpha (source alpha * tint alpha * alpha multiplier)
        let final_alpha = src_a * tint_a;

        if final_alpha >= 1.0 {
          // Fully opaque after modulation
          frame[dst_idx] = tinted_r as u8;
          frame[dst_idx + 1] = tinted_g as u8;
          frame[dst_idx + 2] = tinted_b as u8;
          frame[dst_idx + 3] = 255;
        } else if final_alpha > 0.0 {
          // Alpha blend
          let alpha_i = (final_alpha * 255.0) as u16;
          let inv_alpha = 255 - alpha_i;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = ((tinted_r as u16 * alpha_i + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = ((tinted_g as u16 * alpha_i + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = ((tinted_b as u16 * alpha_i + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
      }
    }
  }

  /// Draw the sprite with rotation, scaling, flipping, and tint/alpha modulation
  ///
  /// This is the most complete draw method, supporting all transform and color
  /// modulation options. Used by the engine to render sprites with tweened
  /// alpha and tint values.
  ///
  /// # Arguments
  ///
  /// * `alpha` - Alpha multiplier (0.0 = invisible, 1.0 = fully opaque)
  /// * `tint` - Color multiplier applied to RGB channels
  #[allow(clippy::too_many_arguments)]
  pub fn draw_rotated_tinted(
    &self,
    frame: &mut [u8],
    frame_width: u32,
    frame_height: u32,
    x: i32,
    y: i32,
    scale_x: f32,
    scale_y: f32,
    rotation_degrees: f32,
    flip_x: bool,
    flip_y: bool,
    alpha: f32,
    tint: Color,
  ) {
    // Early exit for fully transparent
    if alpha <= 0.0 {
      return;
    }

    // Early exit for zero rotation - use faster path
    if rotation_degrees.abs() < 0.01 {
      return self.draw_scaled_xy_flipped_tinted(
        frame,
        frame_width,
        frame_height,
        x,
        y,
        scale_x,
        scale_y,
        flip_x,
        flip_y,
        alpha,
        tint,
      );
    }

    if scale_x <= 0.0 || scale_y <= 0.0 {
      return;
    }

    // Pre-calculate tint multipliers (0.0 - 1.0)
    let tint_r = tint.r as f32 / 255.0;
    let tint_g = tint.g as f32 / 255.0;
    let tint_b = tint.b as f32 / 255.0;
    let tint_a = (tint.a as f32 / 255.0) * alpha;

    // Use ceiling for consistent edge handling
    let scaled_w = (self.width as f32 * scale_x).ceil() as i32;
    let scaled_h = (self.height as f32 * scale_y).ceil() as i32;

    // Rotation center (center of scaled sprite in screen space)
    let center_x = x + scaled_w / 2;
    let center_y = y + scaled_h / 2;

    // Convert to radians
    let angle = rotation_degrees.to_radians();
    let cos_a = angle.cos();
    let sin_a = angle.sin();

    // Calculate bounding box of rotated sprite
    let half_w = scaled_w as f32 / 2.0;
    let half_h = scaled_h as f32 / 2.0;
    let corners = [
      (-half_w, -half_h),
      (half_w, -half_h),
      (-half_w, half_h),
      (half_w, half_h),
    ];

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for (cx, cy) in corners {
      let rx = cx * cos_a - cy * sin_a;
      let ry = cx * sin_a + cy * cos_a;
      min_x = min_x.min(rx);
      max_x = max_x.max(rx);
      min_y = min_y.min(ry);
      max_y = max_y.max(ry);
    }

    let bbox_x1 = (center_x as f32 + min_x).floor() as i32;
    let bbox_y1 = (center_y as f32 + min_y).floor() as i32;
    let bbox_x2 = (center_x as f32 + max_x).ceil() as i32;
    let bbox_y2 = (center_y as f32 + max_y).ceil() as i32;

    // Clip to frame bounds
    let start_x = bbox_x1.max(0);
    let start_y = bbox_y1.max(0);
    let end_x = bbox_x2.min(frame_width as i32);
    let end_y = bbox_y2.min(frame_height as i32);

    if start_x >= end_x || start_y >= end_y {
      return;
    }

    // Inverse rotation matrix for reverse mapping
    let inv_cos = cos_a;
    let inv_sin = -sin_a;

    for py in start_y..end_y {
      for px in start_x..end_x {
        // Sample at pixel center for better quality (0.5 offset)
        let dx = (px - center_x) as f32 + 0.5;
        let dy = (py - center_y) as f32 + 0.5;

        // Rotate backwards (inverse transform)
        let src_x = dx * inv_cos - dy * inv_sin;
        let src_y = dx * inv_sin + dy * inv_cos;

        // Map back to sprite coordinates (accounting for scale)
        let sprite_x_f = src_x / scale_x + (self.width as f32 / 2.0);
        let sprite_y_f = src_y / scale_y + (self.height as f32 / 2.0);

        // Check bounds (with margin for bilinear sampling)
        if sprite_x_f < 0.0
          || sprite_x_f >= (self.width - 1) as f32
          || sprite_y_f < 0.0
          || sprite_y_f >= (self.height - 1) as f32
        {
          continue;
        }

        // Bilinear interpolation for smoother rotation
        let x0 = sprite_x_f.floor() as u32;
        let y0 = sprite_y_f.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);

        // Interpolation weights
        let fx = sprite_x_f - x0 as f32;
        let fy = sprite_y_f - y0 as f32;
        let fx_inv = 1.0 - fx;
        let fy_inv = 1.0 - fy;

        // Apply flipping to all four sample points
        let (sx0, sx1) = if flip_x {
          ((self.width - 1) - x1, (self.width - 1) - x0)
        } else {
          (x0, x1)
        };
        let (sy0, sy1) = if flip_y {
          ((self.height - 1) - y1, (self.height - 1) - y0)
        } else {
          (y0, y1)
        };

        // Sample four pixels
        let idx00 = ((sy0 * self.width + sx0) * 4) as usize;
        let idx10 = ((sy0 * self.width + sx1) * 4) as usize;
        let idx01 = ((sy1 * self.width + sx0) * 4) as usize;
        let idx11 = ((sy1 * self.width + sx1) * 4) as usize;

        // Premultiplied alpha bilinear interpolation
        // This prevents black fringing at edges by weighting RGB by alpha
        let w00 = fx_inv * fy_inv;
        let w10 = fx * fy_inv;
        let w01 = fx_inv * fy;
        let w11 = fx * fy;

        // Get alpha values for each sample (0.0 - 1.0)
        let a00 = self.data[idx00 + 3] as f32 / 255.0;
        let a10 = self.data[idx10 + 3] as f32 / 255.0;
        let a01 = self.data[idx01 + 3] as f32 / 255.0;
        let a11 = self.data[idx11 + 3] as f32 / 255.0;

        // Interpolate premultiplied RGB (RGB * alpha)
        let mut pr = 0.0;
        let mut pg = 0.0;
        let mut pb = 0.0;
        let mut a = 0.0;

        pr += self.data[idx00] as f32 * a00 * w00;
        pg += self.data[idx00 + 1] as f32 * a00 * w00;
        pb += self.data[idx00 + 2] as f32 * a00 * w00;
        a += a00 * w00;

        pr += self.data[idx10] as f32 * a10 * w10;
        pg += self.data[idx10 + 1] as f32 * a10 * w10;
        pb += self.data[idx10 + 2] as f32 * a10 * w10;
        a += a10 * w10;

        pr += self.data[idx01] as f32 * a01 * w01;
        pg += self.data[idx01 + 1] as f32 * a01 * w01;
        pb += self.data[idx01 + 2] as f32 * a01 * w01;
        a += a01 * w01;

        pr += self.data[idx11] as f32 * a11 * w11;
        pg += self.data[idx11 + 1] as f32 * a11 * w11;
        pb += self.data[idx11 + 2] as f32 * a11 * w11;
        a += a11 * w11;

        // Combined alpha (interpolated source alpha * tint alpha)
        let final_alpha = a * tint_a;

        // Skip pixels with very low alpha to prevent edge artifacts
        if final_alpha < 0.1 {
          continue;
        }

        // Unpremultiply to get final RGB, then apply tint
        let r = ((pr / a) * tint_r).min(255.0);
        let g = ((pg / a) * tint_g).min(255.0);
        let b = ((pb / a) * tint_b).min(255.0);

        let dst_idx = (py as usize * frame_width as usize + px as usize) * 4;

        if final_alpha >= 1.0 {
          frame[dst_idx] = r as u8;
          frame[dst_idx + 1] = g as u8;
          frame[dst_idx + 2] = b as u8;
          frame[dst_idx + 3] = 255;
        } else {
          let alpha_i = (final_alpha * 255.0) as u16;
          let inv_alpha = 255 - alpha_i;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = ((r as u16 * alpha_i + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = ((g as u16 * alpha_i + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = ((b as u16 * alpha_i + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
      }
    }
  }
}

/// A simple animated sprite using a sprite sheet
#[derive(Debug, Clone)]
pub struct AnimatedSprite {
  /// The sprite sheet containing all frames
  sheet: Sprite,
  /// Width of each frame
  frame_width: u32,
  /// Height of each frame
  frame_height: u32,
  /// Number of frames in the animation
  frame_count: u32,
  /// Current frame index
  current_frame: u32,
  /// Time accumulator for animation timing
  time_accumulator: f32,
  /// Duration of each frame in seconds
  frame_duration: f32,
}

impl AnimatedSprite {
  /// Create an animated sprite from a horizontal sprite sheet
  ///
  /// # Arguments
  /// * `sheet` - The sprite sheet (frames arranged horizontally)
  /// * `frame_count` - Number of frames in the sheet
  /// * `fps` - Animation frames per second
  pub fn from_sheet(sheet: Sprite, frame_count: u32, fps: f32) -> Self {
    let frame_width = sheet.width() / frame_count;
    let frame_height = sheet.height();

    AnimatedSprite {
      sheet,
      frame_width,
      frame_height,
      frame_count,
      current_frame: 0,
      time_accumulator: 0.0,
      frame_duration: 1.0 / fps,
    }
  }

  /// Update the animation (call this with delta time)
  pub fn update(&mut self, dt: f32) {
    self.time_accumulator += dt;
    while self.time_accumulator >= self.frame_duration {
      self.time_accumulator -= self.frame_duration;
      self.current_frame = (self.current_frame + 1) % self.frame_count;
    }
  }

  /// Draw the current frame to the pixel buffer
  pub fn draw(&self, frame: &mut [u8], frame_width: u32, frame_height: u32, x: i32, y: i32) {
    let frame_w = frame_width as i32;
    let frame_h = frame_height as i32;
    let sprite_w = self.frame_width as i32;
    let sprite_h = self.frame_height as i32;

    // Source offset in sprite sheet
    let sheet_offset_x = (self.current_frame * self.frame_width) as i32;

    // Calculate visible region
    let start_sx = (-x).max(0);
    let start_sy = (-y).max(0);
    let end_sx = (frame_w - x).min(sprite_w);
    let end_sy = (frame_h - y).min(sprite_h);

    if start_sx >= end_sx || start_sy >= end_sy {
      return;
    }

    for sy in start_sy..end_sy {
      for sx in start_sx..end_sx {
        let fx = (x + sx) as usize;
        let fy = (y + sy) as usize;

        let sheet_x = (sheet_offset_x + sx) as u32;
        let sheet_y = sy as u32;

        let src_idx = ((sheet_y * self.sheet.width + sheet_x) * 4) as usize;
        let dst_idx = (fy * frame_width as usize + fx) * 4;

        let src_a = self.sheet.data[src_idx + 3];

        if src_a == 255 {
          frame[dst_idx..dst_idx + 4].copy_from_slice(&self.sheet.data[src_idx..src_idx + 4]);
        } else if src_a > 0 {
          let src_r = self.sheet.data[src_idx] as u16;
          let src_g = self.sheet.data[src_idx + 1] as u16;
          let src_b = self.sheet.data[src_idx + 2] as u16;
          let alpha = src_a as u16;
          let inv_alpha = 255 - alpha;

          let dst_r = frame[dst_idx] as u16;
          let dst_g = frame[dst_idx + 1] as u16;
          let dst_b = frame[dst_idx + 2] as u16;

          frame[dst_idx] = ((src_r * alpha + dst_r * inv_alpha) / 255) as u8;
          frame[dst_idx + 1] = ((src_g * alpha + dst_g * inv_alpha) / 255) as u8;
          frame[dst_idx + 2] = ((src_b * alpha + dst_b * inv_alpha) / 255) as u8;
          frame[dst_idx + 3] = 255;
        }
      }
    }
  }

  /// Get the frame dimensions
  pub fn frame_size(&self) -> (u32, u32) {
    (self.frame_width, self.frame_height)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn solid_rect_creates_correct_size() {
    let sprite = Sprite::solid_rect(10, 20, 255, 0, 0, 255);
    assert_eq!(sprite.width(), 10);
    assert_eq!(sprite.height(), 20);
  }

  #[test]
  fn solid_rect_fills_with_color() {
    let sprite = Sprite::solid_rect(2, 2, 100, 150, 200, 255);
    // Check first pixel
    assert_eq!(sprite.data[0], 100); // R
    assert_eq!(sprite.data[1], 150); // G
    assert_eq!(sprite.data[2], 200); // B
    assert_eq!(sprite.data[3], 255); // A
  }

  #[test]
  fn solid_circle_transparent_pixels_have_correct_rgb() {
    // Create a white circle
    let sprite = Sprite::solid_circle(25, 255, 200, 100, 255);

    // Corner pixel (0,0) should be transparent but have the circle's RGB
    let corner_idx = 0;
    assert_eq!(sprite.data[corner_idx], 255, "Corner R should be 255");
    assert_eq!(sprite.data[corner_idx + 1], 200, "Corner G should be 200");
    assert_eq!(sprite.data[corner_idx + 2], 100, "Corner B should be 100");
    assert_eq!(
      sprite.data[corner_idx + 3],
      0,
      "Corner A should be 0 (transparent)"
    );

    // Center pixel (12,12) should be fully opaque with the circle's RGB
    let center_idx = (12 * 25 + 12) * 4;
    assert_eq!(sprite.data[center_idx], 255, "Center R should be 255");
    assert_eq!(sprite.data[center_idx + 1], 200, "Center G should be 200");
    assert_eq!(sprite.data[center_idx + 2], 100, "Center B should be 100");
    assert_eq!(
      sprite.data[center_idx + 3],
      255,
      "Center A should be 255 (opaque)"
    );
  }

  #[test]
  fn from_rgba_validates_size() {
    let data = vec![0u8; 4 * 4 * 4]; // 4x4 sprite
    let sprite = Sprite::from_rgba(data, 4, 4);
    assert_eq!(sprite.width(), 4);
    assert_eq!(sprite.height(), 4);
  }

  #[test]
  #[should_panic(expected = "Data length must match")]
  fn from_rgba_panics_on_wrong_size() {
    let data = vec![0u8; 10]; // Wrong size
    Sprite::from_rgba(data, 4, 4);
  }

  #[test]
  fn draw_opaque_sprite() {
    let sprite = Sprite::solid_rect(2, 2, 255, 0, 0, 255);
    let mut frame = vec![0u8; 4 * 4 * 4]; // 4x4 frame, all black

    sprite.draw(&mut frame, 4, 4, 1, 1);

    // Check pixel at (1, 1) - should be red
    // Index calculation: (y * width + x) * 4 (RGBA)
    let idx = (4 + 1) * 4;
    assert_eq!(frame[idx], 255); // R
    assert_eq!(frame[idx + 1], 0); // G
    assert_eq!(frame[idx + 2], 0); // B

    // Check pixel at (0, 0) - should still be black
    assert_eq!(frame[0], 0);
  }

  #[test]
  fn draw_clips_to_frame_bounds() {
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 2 * 2 * 4]; // 2x2 frame

    // Draw sprite at (-1, -1), only bottom-right 1x1 should appear at (0,0)
    sprite.draw(&mut frame, 2, 2, -1, -1);

    // Pixel at (0, 0) should be white
    assert_eq!(frame[0], 255);
  }

  #[test]
  fn draw_offscreen_does_nothing() {
    let sprite = Sprite::solid_rect(2, 2, 255, 0, 0, 255);
    let mut frame = vec![0u8; 4 * 4 * 4];
    let original = frame.clone();

    sprite.draw(&mut frame, 4, 4, 100, 100); // Way off screen

    assert_eq!(frame, original);
  }

  #[test]
  fn draw_transparent_sprite_is_skipped() {
    let sprite = Sprite::solid_rect(2, 2, 255, 0, 0, 0); // Alpha = 0
    let mut frame = vec![128u8; 4 * 4 * 4]; // Gray background
    let original = frame.clone();

    sprite.draw(&mut frame, 4, 4, 0, 0);

    assert_eq!(frame, original); // Should be unchanged
  }

  #[test]
  fn animated_sprite_advances_frames() {
    let sheet = Sprite::solid_rect(30, 10, 255, 255, 255, 255); // 3 frames of 10x10
    let mut anim = AnimatedSprite::from_sheet(sheet, 3, 10.0); // 10 FPS

    assert_eq!(anim.current_frame, 0);

    anim.update(0.1); // 100ms = 1 frame at 10 FPS
    assert_eq!(anim.current_frame, 1);

    anim.update(0.2); // 200ms = 2 more frames
    assert_eq!(anim.current_frame, 0); // Wrapped around (1 + 2) % 3 = 0
  }

  #[test]
  fn draw_rotated_zero_degrees() {
    // Rotation of 0 degrees should match non-rotated rendering
    let sprite = Sprite::solid_rect(4, 4, 255, 0, 0, 255);
    let mut frame_rotated = vec![0u8; 10 * 10 * 4];
    let mut frame_normal = vec![0u8; 10 * 10 * 4];

    sprite.draw_rotated(
      &mut frame_rotated,
      10,
      10,
      3,
      3,
      1.0,
      1.0,
      0.0,
      false,
      false,
    );
    sprite.draw_scaled_xy(&mut frame_normal, 10, 10, 3, 3, 1.0, 1.0);

    // Should be identical (or very close due to bilinear filtering)
    let mut diff_count = 0;
    for i in (0..frame_rotated.len()).step_by(4) {
      if frame_rotated[i] != frame_normal[i] {
        diff_count += 1;
      }
    }
    // Allow small differences due to interpolation
    assert!(diff_count < 5, "Too many pixel differences: {}", diff_count);
  }

  #[test]
  fn draw_rotated_90_degrees() {
    // Create a small sprite with distinct red color
    let sprite = Sprite::solid_rect(3, 3, 255, 0, 0, 255);
    let mut frame = vec![0u8; 20 * 20 * 4];

    // Rotate 90 degrees
    sprite.draw_rotated(&mut frame, 20, 20, 8, 8, 1.0, 1.0, 90.0, false, false);

    // After any rotation, we should have some red pixels
    let mut found_red = false;
    for idx in (0..frame.len()).step_by(4) {
      if frame[idx] > 200 && frame[idx + 1] < 50 && frame[idx + 2] < 50 {
        found_red = true;
        break;
      }
    }
    assert!(found_red, "Should find red pixels after 90-degree rotation");
  }

  #[test]
  fn draw_rotated_180_degrees() {
    // Simple test - just verify rotation works
    let sprite = Sprite::solid_rect(3, 3, 255, 0, 0, 255);
    let mut frame = vec![0u8; 20 * 20 * 4];

    sprite.draw_rotated(&mut frame, 20, 20, 8, 8, 1.0, 1.0, 180.0, false, false);

    // Should have red pixels somewhere
    let mut red_count = 0;
    for idx in (0..frame.len()).step_by(4) {
      if frame[idx] > 150 && frame[idx + 1] < 50 && frame[idx + 2] < 50 {
        red_count += 1;
      }
    }
    assert!(
      red_count > 0,
      "Should find red pixels after 180-degree rotation"
    );
  }

  #[test]
  fn draw_rotated_with_scaling() {
    let sprite = Sprite::solid_rect(2, 2, 0, 255, 0, 255); // Green
    let mut frame = vec![0u8; 20 * 20 * 4];

    // Rotate 45 degrees with 3x scaling for more visible result
    sprite.draw_rotated(&mut frame, 20, 20, 6, 6, 3.0, 3.0, 45.0, false, false);

    // Check that green pixels exist
    let mut green_count = 0;
    for idx in (0..frame.len()).step_by(4) {
      if frame[idx + 1] > 150 && frame[idx] < 50 && frame[idx + 2] < 50 {
        green_count += 1;
      }
    }
    assert!(
      green_count > 5,
      "Should have green pixels from scaled rotation, got {}",
      green_count
    );
  }

  #[test]
  fn draw_rotated_with_flip_x() {
    // Simple flip test - just verify it doesn't crash and renders something
    let sprite = Sprite::solid_rect(4, 4, 255, 0, 0, 255);
    let mut frame = vec![0u8; 20 * 20 * 4];

    // Draw with horizontal flip
    sprite.draw_rotated(&mut frame, 20, 20, 8, 8, 1.0, 1.0, 0.0, true, false);

    // Should have some red pixels
    let mut red_count = 0;
    for idx in (0..frame.len()).step_by(4) {
      if frame[idx] > 200 {
        red_count += 1;
      }
    }
    assert!(red_count > 5, "Should have red pixels with flip_x");
  }

  #[test]
  fn draw_rotated_with_flip_y() {
    // Simple flip test - just verify it doesn't crash and renders something
    let sprite = Sprite::solid_rect(4, 4, 255, 0, 0, 255);
    let mut frame = vec![0u8; 20 * 20 * 4];

    // Draw with vertical flip
    sprite.draw_rotated(&mut frame, 20, 20, 8, 8, 1.0, 1.0, 0.0, false, true);

    // Should have some red pixels
    let mut red_count = 0;
    for idx in (0..frame.len()).step_by(4) {
      if frame[idx] > 200 {
        red_count += 1;
      }
    }
    assert!(red_count > 5, "Should have red pixels with flip_y");
  }

  #[test]
  fn draw_rotated_clips_to_bounds() {
    let sprite = Sprite::solid_rect(5, 5, 255, 255, 255, 255);
    let mut frame = vec![0u8; 10 * 10 * 4];

    // Draw partially off-screen
    sprite.draw_rotated(&mut frame, 10, 10, -2, -2, 1.0, 1.0, 45.0, false, false);

    // Should not panic and should clip properly
    // Just verify no out-of-bounds access occurred
  }

  #[test]
  fn draw_rotated_offscreen_does_nothing() {
    let sprite = Sprite::solid_rect(3, 3, 255, 0, 0, 255);
    let mut frame = vec![128u8; 10 * 10 * 4];
    let original = frame.clone();

    // Draw completely off-screen
    sprite.draw_rotated(&mut frame, 10, 10, 100, 100, 1.0, 1.0, 30.0, false, false);

    assert_eq!(
      frame, original,
      "Frame should be unchanged when drawing off-screen"
    );
  }

  #[test]
  fn draw_rotated_alpha_blending() {
    // Create semi-transparent red sprite
    let sprite = Sprite::solid_rect(4, 4, 255, 0, 0, 128); // 50% alpha

    // Green background
    let mut frame = vec![0u8; 10 * 10 * 4];
    for i in (0..frame.len()).step_by(4) {
      frame[i + 1] = 255; // Green
      frame[i + 3] = 255; // Opaque
    }

    sprite.draw_rotated(&mut frame, 10, 10, 3, 3, 1.0, 1.0, 0.0, false, false);

    // Check center pixel - should be blend of red and green
    let idx = (5 * 10 + 5) * 4;
    let r = frame[idx];
    let g = frame[idx + 1];

    // Should have both red and green components (blending)
    assert!(r > 50, "Should have significant red component");
    assert!(g > 50, "Should have significant green component");
    assert!(
      r > g,
      "Red should be stronger than green due to red overlay"
    );
  }

  #[test]
  fn draw_rotated_small_angle_smooth() {
    // Test that small rotations work without crashing
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 20 * 20 * 4];

    // Rotate by small angle
    sprite.draw_rotated(&mut frame, 20, 20, 8, 8, 1.0, 1.0, 5.0, false, false);

    // Count white pixels
    let mut white_count = 0;
    for idx in (0..frame.len()).step_by(4) {
      if frame[idx] > 200 && frame[idx + 1] > 200 && frame[idx + 2] > 200 {
        white_count += 1;
      }
    }
    assert!(
      white_count > 2,
      "Should have white pixels from small rotation, got {}",
      white_count
    );
  }

  // =========================================================================
  // Tinted Drawing Tests
  // =========================================================================

  #[test]
  fn draw_tinted_with_half_alpha() {
    // White sprite drawn at 50% alpha on green background should blend
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 10 * 10 * 4];

    // Fill with green background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 0; // R
      frame[i + 1] = 255; // G
      frame[i + 2] = 0; // B
      frame[i + 3] = 255; // A
    }

    // Draw with 50% alpha
    sprite.draw_scaled_xy_flipped_tinted(
      &mut frame,
      10,
      10,
      3,
      3,
      1.0,
      1.0,
      false,
      false,
      0.5,
      Color::WHITE,
    );

    // Check a pixel in the sprite area - should be blended (greenish-white)
    let idx = (4 * 10 + 4) * 4; // Center-ish pixel
    let r = frame[idx];
    let g = frame[idx + 1];
    let b = frame[idx + 2];

    // Should have significant red (from white) and green (from background)
    assert!(
      r > 100,
      "Red should be present from white sprite, got {}",
      r
    );
    assert!(
      g > 100,
      "Green should be present from background, got {}",
      g
    );
    // Blue should be roughly half of white (127-ish)
    assert!(b > 50 && b < 200, "Blue should be blended, got {}", b);
  }

  #[test]
  fn draw_tinted_with_red_tint() {
    // White sprite tinted red should appear red
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 10 * 10 * 4];

    // Draw with red tint
    sprite.draw_scaled_xy_flipped_tinted(
      &mut frame,
      10,
      10,
      3,
      3,
      1.0,
      1.0,
      false,
      false,
      1.0,
      Color::new(255, 0, 0, 255),
    );

    // Check a pixel in the sprite area
    let idx = (4 * 10 + 4) * 4;
    let r = frame[idx];
    let g = frame[idx + 1];
    let b = frame[idx + 2];

    // Should be red (high R, low G and B)
    assert_eq!(r, 255, "Red should be full");
    assert_eq!(g, 0, "Green should be zero from red tint");
    assert_eq!(b, 0, "Blue should be zero from red tint");
  }

  #[test]
  fn draw_tinted_with_zero_alpha_draws_nothing() {
    // Zero alpha should not modify the frame
    let sprite = Sprite::solid_rect(4, 4, 255, 0, 0, 255);
    let mut frame = vec![128u8; 10 * 10 * 4];
    let original = frame.clone();

    sprite.draw_scaled_xy_flipped_tinted(
      &mut frame,
      10,
      10,
      3,
      3,
      1.0,
      1.0,
      false,
      false,
      0.0,
      Color::WHITE,
    );

    assert_eq!(frame, original, "Zero alpha should not change frame");
  }

  #[test]
  fn draw_tinted_darkening() {
    // White sprite with gray tint should appear gray (darkened)
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 10 * 10 * 4];

    // Draw with 50% gray tint (darkening)
    sprite.draw_scaled_xy_flipped_tinted(
      &mut frame,
      10,
      10,
      3,
      3,
      1.0,
      1.0,
      false,
      false,
      1.0,
      Color::new(128, 128, 128, 255),
    );

    let idx = (4 * 10 + 4) * 4;
    let r = frame[idx];
    let g = frame[idx + 1];
    let b = frame[idx + 2];

    // Should be approximately half brightness (128)
    assert!(
      r > 120 && r < 136,
      "Red should be ~128 from gray tint, got {}",
      r
    );
    assert!(
      g > 120 && g < 136,
      "Green should be ~128 from gray tint, got {}",
      g
    );
    assert!(
      b > 120 && b < 136,
      "Blue should be ~128 from gray tint, got {}",
      b
    );
  }

  #[test]
  fn draw_rotated_tinted_with_alpha() {
    // Test rotated tinted drawing
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 20 * 20 * 4];

    // Fill with blue background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 0;
      frame[i + 1] = 0;
      frame[i + 2] = 255;
      frame[i + 3] = 255;
    }

    // Draw rotated 45 degrees with 50% alpha and green tint
    sprite.draw_rotated_tinted(
      &mut frame,
      20,
      20,
      8,
      8,
      1.0,
      1.0,
      45.0,
      false,
      false,
      0.5,
      Color::new(0, 255, 0, 255),
    );

    // Find pixels that were modified (not pure blue)
    let mut found_blended = false;
    for idx in (0..frame.len()).step_by(4) {
      let g = frame[idx + 1];
      let b = frame[idx + 2];

      // Look for pixels that have green from tint blended with blue background
      if g > 50 && b > 50 && g < 255 && b < 255 {
        found_blended = true;
        break;
      }
    }

    assert!(
      found_blended,
      "Should find blended green+blue pixels from tinted rotated sprite"
    );
  }

  #[test]
  fn draw_tinted_combines_source_and_tint_alpha() {
    // Semi-transparent sprite with tint alpha should multiply
    let sprite = Sprite::solid_rect(4, 4, 255, 255, 255, 128); // 50% source alpha
    let mut frame = vec![0u8; 10 * 10 * 4];

    // Fill with green background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 0;
      frame[i + 1] = 255;
      frame[i + 2] = 0;
      frame[i + 3] = 255;
    }

    // Draw with 50% tint alpha (combined = 25% final alpha)
    sprite.draw_scaled_xy_flipped_tinted(
      &mut frame,
      10,
      10,
      3,
      3,
      1.0,
      1.0,
      false,
      false,
      0.5,
      Color::new(255, 255, 255, 255),
    );

    let idx = (4 * 10 + 4) * 4;
    let g = frame[idx + 1];

    // Green should still be quite high since only 25% white was blended
    assert!(
      g > 150,
      "Green should be mostly preserved with 25% blend, got {}",
      g
    );
  }

  // =========================================================================
  // Premultiplied Alpha / Black Fringing Tests
  // =========================================================================

  #[test]
  fn rotated_circle_no_black_fringing() {
    // A white circle rotated on a green background should NOT produce black pixels
    // This tests that premultiplied alpha interpolation works correctly
    let circle = Sprite::solid_circle(20, 255, 255, 255, 255);
    let mut frame = vec![0u8; 40 * 40 * 4];

    // Fill with bright green background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 0; // R
      frame[i + 1] = 255; // G
      frame[i + 2] = 0; // B
      frame[i + 3] = 255; // A
    }

    // Draw the circle rotated (rotation triggers bilinear interpolation)
    circle.draw_rotated(&mut frame, 40, 40, 10, 10, 1.0, 1.0, 30.0, false, false);

    // Check for any black or near-black pixels that shouldn't exist
    // Black fringing would show as pixels with very low RGB but some alpha
    let mut black_pixel_count = 0;
    for i in (0..frame.len()).step_by(4) {
      let r = frame[i];
      let g = frame[i + 1];
      let b = frame[i + 2];

      // A "black fringe" pixel would be dark (low RGB) but not pure background green
      // and not the expected white from the circle
      if r < 30 && g < 30 && b < 30 {
        black_pixel_count += 1;
      }
    }

    assert_eq!(
      black_pixel_count, 0,
      "Should have no black fringing pixels around rotated circle, found {}",
      black_pixel_count
    );
  }

  #[test]
  fn rotated_circle_tinted_no_black_fringing() {
    // Same test but with tinted drawing
    let circle = Sprite::solid_circle(20, 255, 255, 255, 255);
    let mut frame = vec![0u8; 40 * 40 * 4];

    // Fill with bright blue background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 0;
      frame[i + 1] = 0;
      frame[i + 2] = 255;
      frame[i + 3] = 255;
    }

    // Draw the circle rotated with red tint
    circle.draw_rotated_tinted(
      &mut frame,
      40,
      40,
      10,
      10,
      1.0,
      1.0,
      45.0,
      false,
      false,
      1.0,
      Color::new(255, 0, 0, 255),
    );

    // Check for black fringing
    let mut black_pixel_count = 0;
    for i in (0..frame.len()).step_by(4) {
      let r = frame[i];
      let g = frame[i + 1];
      let b = frame[i + 2];

      if r < 30 && g < 30 && b < 30 {
        black_pixel_count += 1;
      }
    }

    assert_eq!(
      black_pixel_count, 0,
      "Should have no black fringing pixels around tinted rotated circle, found {}",
      black_pixel_count
    );
  }

  #[test]
  fn scaled_rotated_circle_no_black_fringing() {
    // Test with scaling which stresses the interpolation more
    let circle = Sprite::solid_circle(15, 200, 150, 100, 255); // Brown-ish circle
    let mut frame = vec![0u8; 60 * 60 * 4];

    // Fill with cyan background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 0;
      frame[i + 1] = 255;
      frame[i + 2] = 255;
      frame[i + 3] = 255;
    }

    // Draw scaled up and rotated
    circle.draw_rotated(&mut frame, 60, 60, 10, 10, 2.5, 2.5, 67.0, false, false);

    // Check for black fringing
    let mut black_pixel_count = 0;
    for i in (0..frame.len()).step_by(4) {
      let r = frame[i];
      let g = frame[i + 1];
      let b = frame[i + 2];

      if r < 30 && g < 30 && b < 30 {
        black_pixel_count += 1;
      }
    }

    assert_eq!(
      black_pixel_count, 0,
      "Should have no black fringing on scaled rotated circle, found {}",
      black_pixel_count
    );
  }

  #[test]
  fn circle_edge_preserves_color_during_rotation() {
    // Verify that edge pixels of a colored circle maintain their hue when rotated
    // (not turning black or getting color-shifted)
    let circle = Sprite::solid_circle(30, 255, 128, 0, 255); // Orange circle
    let mut frame = vec![0u8; 50 * 50 * 4];

    // White background
    for i in (0..frame.len()).step_by(4) {
      frame[i] = 255;
      frame[i + 1] = 255;
      frame[i + 2] = 255;
      frame[i + 3] = 255;
    }

    // Draw rotated
    circle.draw_rotated(&mut frame, 50, 50, 10, 10, 1.0, 1.0, 22.0, false, false);

    // Find pixels that are partially orange (edge pixels from blending)
    // These should have R > G (orange-ish) not R == G == B (gray/black fringing)
    let mut found_orange_blend = false;
    for i in (0..frame.len()).step_by(4) {
      let r = frame[i];
      let g = frame[i + 1];
      let b = frame[i + 2];

      // Look for blended edge pixels (not pure white, not pure orange)
      // Should have orange hue: R > G > B
      if r > 200 && g > 100 && g < 200 && b < g {
        found_orange_blend = true;
        break;
      }
    }

    assert!(
      found_orange_blend,
      "Should find orange-tinted edge pixels (proper color blending, not black fringing)"
    );
  }
}
