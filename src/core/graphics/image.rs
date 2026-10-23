use std::sync::{Arc, OnceLock};

use crate::core::asset::Asset;
use crate::core::error::ImageError;

/// An image's pixels: RGBA, 4 bytes per pixel, in rows from the top.
///
/// An image is an asset. Load one from a file with the
/// [`ImageLoader`](crate::core::asset::ImageLoader)
/// (`ctx.assets().load_now::<Image>("ship.png")`), or make one in code and
/// add it with `ctx.assets().add(image)`. Clones share their pixels until
/// one of them changes its own ([`Image::data_mut`])
#[derive(Debug, Clone)]
pub struct Image {
  /// Raw RGBA pixel data (4 bytes per pixel), shared with clones and the renderer
  data: Arc<Vec<u8>>,
  /// Width in pixels
  width: u32,
  /// Height in pixels
  height: u32,
  /// Whether all pixels are fully opaque (alpha=255), enabling fast row-copy
  /// drawing; worked out again on first ask after the pixels change
  opaque: OnceLock<bool>,
  /// The mip levels below the first of the image's own texture, made by the
  /// loader for an image too large for the atlas's shared pages (see
  /// [`Image::prepare_mips`]); none once the pixels change
  mips: Option<Arc<Vec<Vec<u8>>>>,
}

impl Asset for Image {}

/// The checker's two colours (RGBA)
pub(crate) const CHECKER_MAGENTA: [u8; 4] = [255, 0, 255, 255];
pub(crate) const CHECKER_BLACK: [u8; 4] = [0, 0, 0, 255];

/// The world's images: its `Assets<Image>`, or an empty store for a world
/// without one
pub(crate) fn images_of(world: &crate::core::ecs::World) -> &crate::core::asset::Assets<Image> {
  world
    .get_resource::<crate::core::asset::Assets<Image>>()
    .unwrap_or_else(|| no_images())
}

#[cfg(test)]
thread_local! {
  /// The test thread's own image store (leaked: tests only)
  static TEST_IMAGES: &'static std::cell::RefCell<crate::core::asset::Assets<Image>> =
    Box::leak(Box::default());
}

/// A handle to `image` in the test thread's own store ([`test_images`]):
/// each call is another id
#[cfg(test)]
pub(crate) fn test_handle(image: Image) -> crate::core::asset::Handle<Image> {
  TEST_IMAGES.with(|images| images.borrow_mut().add(image))
}

/// The test thread's own image store, which [`test_handle`] adds to; held,
/// it takes no more
#[cfg(test)]
pub(crate) fn test_images() -> std::cell::Ref<'static, crate::core::asset::Assets<Image>> {
  TEST_IMAGES.with(|images| images.borrow())
}

/// An empty image store, for what reads images where there are none
pub(crate) fn no_images() -> &'static crate::core::asset::Assets<Image> {
  static NONE: OnceLock<crate::core::asset::Assets<Image>> = OnceLock::new();
  NONE.get_or_init(Default::default)
}

impl Image {
  /// Check if all pixels in the data are fully opaque (alpha=255)
  fn check_opaque(data: &[u8]) -> bool {
    data.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255)
  }

  /// An image of `data`, whose opacity is already known
  fn from_parts(data: Vec<u8>, width: u32, height: u32, is_opaque: bool) -> Self {
    Image {
      data: Arc::new(data),
      width,
      height,
      opaque: OnceLock::from(is_opaque),
      mips: None,
    }
  }

  /// The image drawn in place of one that is not loaded, in debug builds:
  /// 16x16 px of magenta and black 4 px squares, stretched over the
  /// sprite's size. The UI draws it only where it is given a size or a
  /// rect: an unloaded image at its natural size (`draw_sprite`, `ui.image`
  /// or `UiNode::image` with no size, a prompt's glyph) is zero-sized and
  /// draws nothing, so a UI lays out the same in debug and release builds
  pub(crate) fn checker() -> Self {
    const SIDE: u32 = 16;
    const SQUARE: u32 = 4;
    let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
      for x in 0..SIDE {
        let magenta = (x / SQUARE + y / SQUARE).is_multiple_of(2);
        data.extend_from_slice(if magenta {
          &CHECKER_MAGENTA
        } else {
          &CHECKER_BLACK
        });
      }
    }
    Self::from_parts(data, SIDE, SIDE, true)
  }

  /// Create a new image from raw RGBA data
  pub fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(
      data.len(),
      (width * height * 4) as usize,
      "Data length must match width * height * 4"
    );
    let is_opaque = Self::check_opaque(&data);
    Self::from_parts(data, width, height, is_opaque)
  }

  /// Decodes an image file's bytes (any raster format the `image` crate is built with)
  pub fn decode(bytes: &[u8]) -> Result<Image, ImageError> {
    Ok(Self::from_decoded(image::load_from_memory(bytes)?))
  }

  /// Decodes bytes in `format`, for a format its bytes do not name (TGA has
  /// no signature to know it by)
  pub(crate) fn decode_as(bytes: &[u8], format: image::ImageFormat) -> Result<Image, ImageError> {
    Ok(Self::from_decoded(image::load_from_memory_with_format(
      bytes, format,
    )?))
  }

  fn from_decoded(decoded: image::DynamicImage) -> Image {
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    let data = rgba.into_raw();
    let is_opaque = Self::check_opaque(&data);
    Self::from_parts(data, width, height, is_opaque)
  }

  /// Create a solid color rectangle image
  pub fn solid_rect(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for pixel in data.as_chunks_mut::<4>().0 {
      pixel[0] = r;
      pixel[1] = g;
      pixel[2] = b;
      pixel[3] = a;
    }
    Self::from_parts(data, width, height, a == 255)
  }

  /// Create a solid color circle image with anti-aliasing
  ///
  /// The image will be `diameter x diameter` pixels, with a circle centered in it.
  /// Note: All pixels (including transparent ones) have the same RGB color to prevent
  /// black fringing during bilinear interpolation in rotated/scaled rendering.
  pub fn solid_circle(diameter: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
    // Initialize all pixels with the target color (not black) to prevent
    // black fringing during bilinear interpolation at edges
    let mut data = vec![0u8; (diameter * diameter * 4) as usize];
    for pixel in data.as_chunks_mut::<4>().0 {
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
    Self::from_parts(data, diameter, diameter, false)
  }

  /// Create a solid-colored ellipse image with anti-aliased edges.
  ///
  /// The image will be `width x height` pixels, with an ellipse filling it.
  /// Useful for blob shadows and other oval shapes.
  ///
  /// Note: All pixels (including transparent ones) have the same RGB color to prevent
  /// black fringing during bilinear interpolation in rotated/scaled rendering.
  pub fn solid_ellipse(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
    // Initialize all pixels with the target color to prevent fringing
    let mut data = vec![0u8; (width * height * 4) as usize];
    for pixel in data.as_chunks_mut::<4>().0 {
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

    Self::from_parts(data, width, height, false)
  }

  /// Get the image width
  pub fn width(&self) -> u32 {
    self.width
  }

  /// Get the image height
  pub fn height(&self) -> u32 {
    self.height
  }

  /// Check if the image is fully opaque (no transparent or semi-transparent pixels)
  pub fn is_opaque(&self) -> bool {
    *self.opaque.get_or_init(|| Self::check_opaque(&self.data))
  }

  /// Get the raw RGBA pixel data
  ///
  /// Returns a slice of the raw pixel data. Each pixel is 4 bytes: R, G, B, A.
  /// The data is stored in row-major order (left to right, top to bottom).
  pub fn data(&self) -> &[u8] {
    &self.data
  }

  /// The pixels for changing in place, laid out as [`Image::data`] gives
  /// them. Clones that share them keep theirs: the pixels are copied first.
  /// An image in an asset store is changed through `get_mut`, which moves
  /// its version on, so what was built from it is built again
  pub fn data_mut(&mut self) -> &mut [u8] {
    self.mips = None;
    self.opaque = OnceLock::new();
    Arc::make_mut(&mut self.data).as_mut_slice()
  }

  /// The pixels, shared: the renderer takes this at extract time
  pub(crate) fn pixels(&self) -> &Arc<Vec<u8>> {
    &self.data
  }

  /// Makes the mip levels below the first of the image's own texture, for
  /// an image too large for the atlas's shared pages (over 256 px on a
  /// side); nothing for a smaller one. The loader calls it on its thread, so
  /// the upload does not make them on the main thread
  pub(crate) fn prepare_mips(&mut self) {
    self.mips = super::atlas::standalone_mips(&self.data, self.width, self.height).map(Arc::new);
  }

  /// The mip levels [`Image::prepare_mips`] made: none for an image in the
  /// atlas's shared pages, one made in code, or one changed since
  pub(crate) fn mips(&self) -> Option<&Arc<Vec<Vec<u8>>>> {
    self.mips.as_ref()
  }

  /// Create a new image with pre-scaled pixel data (nearest-neighbor).
  ///
  /// This is useful for caching scaled versions of images to avoid
  /// expensive per-frame scaling operations.
  ///
  /// # Arguments
  /// * `scale` - Uniform scale factor (1.0 = original size)
  pub fn scaled(&self, scale: f32) -> Self {
    self.scaled_xy(scale, scale)
  }

  /// Create a new image with pre-scaled pixel data (nearest-neighbor, non-uniform).
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

        // Clamp to image bounds
        let sx = sx.min(self.width - 1);
        let sy = sy.min(self.height - 1);

        let src_idx = ((sy * self.width + sx) * 4) as usize;
        let dst_idx = ((dy * new_width + dx) * 4) as usize;

        data[dst_idx..dst_idx + 4].copy_from_slice(&self.data[src_idx..src_idx + 4]);
      }
    }

    // A scaled image inherits opacity from its source (scaling doesn't change alpha)
    Self::from_parts(data, new_width, new_height, self.is_opaque())
  }

  /// Draw the image to a pixel buffer at the given position
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
    let image_w = self.width as i32;
    let image_h = self.height as i32;

    // Calculate visible region (clip to screen bounds)
    let start_sx = (-x).max(0);
    let start_sy = (-y).max(0);
    let end_sx = (frame_w - x).min(image_w);
    let end_sy = (frame_h - y).min(image_h);

    // Early exit if completely off-screen
    if start_sx >= end_sx || start_sy >= end_sy {
      return;
    }

    // Fast path for fully opaque images: use row-based memcpy
    if self.is_opaque() {
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

    // Slow path for images with transparency: per-pixel alpha blending
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

  /// Draw the image scaled (nearest-neighbor) to a pixel buffer
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

  /// Draw the image with non-uniform scaling (nearest-neighbor)
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
        // Map screen pixel back to the source image (nearest neighbor)
        let sx = ((dx as f32) / scale_x) as u32;
        let sy = ((dy as f32) / scale_y) as u32;

        // Clamp to image bounds (safety)
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
}

/// A simple animated sprite using a sprite sheet
#[derive(Debug, Clone)]
pub struct AnimatedSprite {
  /// The sprite sheet containing all frames
  sheet: Image,
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
  pub fn from_sheet(sheet: Image, frame_count: u32, fps: f32) -> Self {
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
    let image = Image::solid_rect(10, 20, 255, 0, 0, 255);
    assert_eq!(image.width(), 10);
    assert_eq!(image.height(), 20);
  }

  #[test]
  fn solid_rect_fills_with_color() {
    let image = Image::solid_rect(2, 2, 100, 150, 200, 255);
    // Check first pixel
    assert_eq!(image.data[0], 100); // R
    assert_eq!(image.data[1], 150); // G
    assert_eq!(image.data[2], 200); // B
    assert_eq!(image.data[3], 255); // A
  }

  #[test]
  fn solid_circle_transparent_pixels_have_correct_rgb() {
    // Create a white circle
    let image = Image::solid_circle(25, 255, 200, 100, 255);

    // Corner pixel (0,0) should be transparent but have the circle's RGB
    let corner_idx = 0;
    assert_eq!(image.data[corner_idx], 255, "Corner R should be 255");
    assert_eq!(image.data[corner_idx + 1], 200, "Corner G should be 200");
    assert_eq!(image.data[corner_idx + 2], 100, "Corner B should be 100");
    assert_eq!(
      image.data[corner_idx + 3],
      0,
      "Corner A should be 0 (transparent)"
    );

    // Center pixel (12,12) should be fully opaque with the circle's RGB
    let center_idx = (12 * 25 + 12) * 4;
    assert_eq!(image.data[center_idx], 255, "Center R should be 255");
    assert_eq!(image.data[center_idx + 1], 200, "Center G should be 200");
    assert_eq!(image.data[center_idx + 2], 100, "Center B should be 100");
    assert_eq!(
      image.data[center_idx + 3],
      255,
      "Center A should be 255 (opaque)"
    );
  }

  #[test]
  fn from_rgba_validates_size() {
    let data = vec![0u8; 4 * 4 * 4]; // 4x4 image
    let image = Image::from_rgba(data, 4, 4);
    assert_eq!(image.width(), 4);
    assert_eq!(image.height(), 4);
  }

  #[test]
  #[should_panic(expected = "Data length must match")]
  fn from_rgba_panics_on_wrong_size() {
    let data = vec![0u8; 10]; // Wrong size
    Image::from_rgba(data, 4, 4);
  }

  #[test]
  fn draw_opaque_image() {
    let image = Image::solid_rect(2, 2, 255, 0, 0, 255);
    let mut frame = vec![0u8; 4 * 4 * 4]; // 4x4 frame, all black

    image.draw(&mut frame, 4, 4, 1, 1);

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
    let image = Image::solid_rect(4, 4, 255, 255, 255, 255);
    let mut frame = vec![0u8; 2 * 2 * 4]; // 2x2 frame

    // Draw the image at (-1, -1), only bottom-right 1x1 should appear at (0,0)
    image.draw(&mut frame, 2, 2, -1, -1);

    // Pixel at (0, 0) should be white
    assert_eq!(frame[0], 255);
  }

  #[test]
  fn draw_offscreen_does_nothing() {
    let image = Image::solid_rect(2, 2, 255, 0, 0, 255);
    let mut frame = vec![0u8; 4 * 4 * 4];
    let original = frame.clone();

    image.draw(&mut frame, 4, 4, 100, 100); // Way off screen

    assert_eq!(frame, original);
  }

  #[test]
  fn draw_transparent_image_is_skipped() {
    let image = Image::solid_rect(2, 2, 255, 0, 0, 0); // Alpha = 0
    let mut frame = vec![128u8; 4 * 4 * 4]; // Gray background
    let original = frame.clone();

    image.draw(&mut frame, 4, 4, 0, 0);

    assert_eq!(frame, original); // Should be unchanged
  }

  /// A 300x300 opaque image with its mips prepared
  fn large() -> Image {
    let mut image = Image::solid_rect(300, 300, 40, 80, 120, 255);
    image.prepare_mips();
    image
  }

  #[test]
  fn editing_the_pixels_drops_the_prepared_mips() {
    let mut image = large();
    assert!(image.mips().is_some(), "300 px is too large for the atlas");
    image.data_mut()[0] = 0;
    assert!(image.mips().is_none(), "the old levels show the old pixels");
  }

  #[test]
  fn editing_a_clone_leaves_the_original() {
    let original = Image::solid_rect(2, 2, 10, 20, 30, 255);
    let mut edited = original.clone();
    assert!(Arc::ptr_eq(original.pixels(), edited.pixels()));
    edited.data_mut()[0] = 99;
    assert_eq!(original.data()[0], 10);
    assert_eq!(edited.data()[0], 99);
  }

  #[test]
  fn an_edit_is_seen_by_is_opaque() {
    let mut image = Image::solid_rect(2, 2, 10, 20, 30, 255);
    assert!(image.is_opaque());
    image.data_mut()[3] = 128;
    assert!(!image.is_opaque(), "one translucent pixel");
    image.data_mut()[3] = 255;
    assert!(image.is_opaque());
  }

  #[test]
  fn decode_reads_any_raster_format() {
    let pixels = image::RgbaImage::from_fn(3, 2, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
    let mut bmp = std::io::Cursor::new(Vec::new());
    if let Err(error) = pixels.write_to(&mut bmp, image::ImageFormat::Bmp) {
      panic!("encoding a BMP: {error}");
    }
    let decoded = match Image::decode(bmp.get_ref()) {
      Ok(decoded) => decoded,
      Err(error) => panic!("{error}"),
    };
    assert_eq!((decoded.width(), decoded.height()), (3, 2));
    assert_eq!(decoded.data(), pixels.as_raw().as_slice());
    assert!(decoded.is_opaque());
    assert!(matches!(
      Image::decode(b"no image"),
      Err(ImageError::Decode(_))
    ));
  }

  #[test]
  fn animated_sprite_advances_frames() {
    let sheet = Image::solid_rect(30, 10, 255, 255, 255, 255); // 3 frames of 10x10
    let mut anim = AnimatedSprite::from_sheet(sheet, 3, 10.0); // 10 FPS

    assert_eq!(anim.current_frame, 0);

    anim.update(0.1); // 100ms = 1 frame at 10 FPS
    assert_eq!(anim.current_frame, 1);

    anim.update(0.2); // 200ms = 2 more frames
    assert_eq!(anim.current_frame, 0); // Wrapped around (1 + 2) % 3 = 0
  }
}
