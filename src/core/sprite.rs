use std::path::Path;

/// A 2D sprite that can be drawn to the screen
#[derive(Debug, Clone)]
pub struct Sprite {
  /// Raw RGBA pixel data (4 bytes per pixel)
  data: Vec<u8>,
  /// Width in pixels
  width: u32,
  /// Height in pixels
  height: u32,
}

impl Sprite {
  /// Create a new sprite from raw RGBA data
  pub fn from_rgba(data: Vec<u8>, width: u32, height: u32) -> Self {
    assert_eq!(
      data.len(),
      (width * height * 4) as usize,
      "Data length must match width * height * 4"
    );
    Sprite {
      data,
      width,
      height,
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
    }
  }

  /// Load a sprite from an image file (PNG, JPEG, etc.)
  pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
    let img = image::open(path.as_ref())
      .map_err(|e| format!("Failed to load image: {}", e))?
      .into_rgba8();

    let width = img.width();
    let height = img.height();
    let data = img.into_raw();

    Ok(Sprite {
      data,
      width,
      height,
    })
  }

  /// Get the sprite width
  pub fn width(&self) -> u32 {
    self.width
  }

  /// Get the sprite height
  pub fn height(&self) -> u32 {
    self.height
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
    if scale <= 0.0 {
      return;
    }

    let frame_w = frame_width as i32;
    let frame_h = frame_height as i32;
    let scaled_w = (self.width as f32 * scale) as i32;
    let scaled_h = (self.height as f32 * scale) as i32;

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
        let sx = ((dx as f32) / scale) as u32;
        let sy = ((dy as f32) / scale) as u32;

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
}
