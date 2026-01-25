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
    Sprite { data, width, height }
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
    Sprite { data, width, height }
  }

  /// Load a sprite from an image file (PNG, JPEG, etc.)
  pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, String> {
    let img = image::open(path.as_ref())
      .map_err(|e| format!("Failed to load image: {}", e))?
      .into_rgba8();

    let width = img.width();
    let height = img.height();
    let data = img.into_raw();

    Ok(Sprite { data, width, height })
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
