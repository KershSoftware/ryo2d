//! Debug visualization and rendering utilities
//!
//! This module provides engine-level debug visualization for development.
//! All debug features can be configured via `Ryo.toml` or the `EngineBuilder`.
//!
//! # Available Debug Options
//!
//! | Option | Description |
//! |--------|-------------|
//! | `show_colliders` | Draw collision boundaries (green outlines) |
//! | `show_velocities` | Draw velocity vectors on physics entities |
//! | `show_origins` | Draw entity origin points (transform position) |
//!
//! # Available Info Overlays
//!
//! | Option | Description |
//! |--------|-------------|
//! | `show_fps` | Display FPS counter overlay |
//!
//! # Ryo.toml Configuration
//!
//! ```toml
//! [debug]
//! show_colliders = true
//! show_velocities = false
//! show_origins = false
//!
//! [info]
//! show_fps = true
//! ```
//!
//! # Programmatic Configuration
//!
//! ```rust,ignore
//! EngineBuilder::new()
//!     .show_colliders(true)
//!     .show_fps(true)
//!     .build()
//! ```
//!
//! Debug settings from `Ryo.toml` are automatically loaded during `build()`.
//! Values set programmatically will override config file settings.

use crate::core::ecs::{Transform, Velocity, World};
use crate::core::graphics::{Camera, GroupedCollider};
use crate::core::physics::{Collider, ColliderShape};
use crate::core::ui::{Color, UiContext};

/// Debug configuration for engine rendering.
///
/// Controls which debug visualizations are enabled.
/// Can be loaded from `Ryo.toml` or set programmatically.
#[derive(Debug, Clone, Default)]
pub struct DebugConfig {
  /// Whether to render collision boundaries (bright green outlines)
  pub show_colliders: bool,
  /// Whether to display FPS counter in corner
  pub show_fps: bool,
  /// Whether to display frame timing breakdown (update/physics/render/present)
  pub show_frame_timing: bool,
  /// Whether to draw velocity vectors on physics entities
  pub show_velocities: bool,
  /// Whether to draw entity origin points (small crosshairs at transform position)
  pub show_origins: bool,
}

// ============================================================================
// Frame Timing
// ============================================================================

use std::time::{Duration, Instant};

/// Tracks timing for each phase of the game loop.
///
/// Used to display frame timing breakdown when `show_frame_timing` is enabled.
/// Measurements have minimal overhead (just Instant::now() calls).
///
/// Note: Currently, `update` includes physics time (combined in fixed timestep loop).
/// The `physics` field is reserved for future separate measurement.
#[derive(Debug, Clone, Default)]
pub struct FrameTimings {
  /// Time spent in game update logic (currently includes physics)
  pub update: Duration,
  /// Time spent in physics simulation (reserved for future use)
  pub physics: Duration,
  /// Time spent rendering (CPU side)
  pub render: Duration,
  /// Time spent presenting to screen (GPU sync)
  pub present: Duration,
  /// Total frame time
  pub total: Duration,
  /// Calculated FPS from total time
  pub fps: u32,
}

impl FrameTimings {
  /// Calculate FPS from total frame time
  pub fn calculate_fps(&mut self) {
    if self.total.as_secs_f32() > 0.0 {
      self.fps = (1.0 / self.total.as_secs_f32()) as u32;
    }
  }
}

/// Smoothed frame timing values for stable overlay display.
///
/// Uses exponential moving average to prevent jittery numbers.
/// The smoothing factor (alpha) controls responsiveness vs stability.
#[derive(Debug, Clone)]
pub struct SmoothedFrameTimings {
  /// Smoothed update time in milliseconds
  pub update_ms: f64,
  /// Smoothed render time in milliseconds
  pub render_ms: f64,
  /// Smoothed present time in milliseconds
  pub present_ms: f64,
  /// Smoothed total time in milliseconds
  pub total_ms: f64,
  /// Smoothed FPS value (from actual measured FPS, not instantaneous)
  pub fps: f64,
  /// Smoothing factor (0.0 = no change, 1.0 = instant update)
  alpha: f64,
}

impl Default for SmoothedFrameTimings {
  fn default() -> Self {
    Self {
      update_ms: 0.0,
      render_ms: 0.0,
      present_ms: 0.0,
      total_ms: 0.0,
      fps: 0.0,
      alpha: 0.02, // Smooth over ~50 frames for stable display
    }
  }
}

impl SmoothedFrameTimings {
  /// Update smoothed values from raw frame timings and actual measured FPS.
  ///
  /// Note: `actual_fps` should come from the time module's measured FPS,
  /// not from instantaneous frame time (which excludes vsync/sleep).
  pub fn update(&mut self, raw: &FrameTimings, actual_fps: u64) {
    let update_ms = raw.update.as_secs_f64() * 1000.0;
    let render_ms = raw.render.as_secs_f64() * 1000.0;
    let present_ms = raw.present.as_secs_f64() * 1000.0;
    let total_ms = raw.total.as_secs_f64() * 1000.0;

    // Exponential moving average: smoothed = alpha * new + (1 - alpha) * old
    self.update_ms = self.alpha * update_ms + (1.0 - self.alpha) * self.update_ms;
    self.render_ms = self.alpha * render_ms + (1.0 - self.alpha) * self.render_ms;
    self.present_ms = self.alpha * present_ms + (1.0 - self.alpha) * self.present_ms;
    self.total_ms = self.alpha * total_ms + (1.0 - self.alpha) * self.total_ms;
    // Use actual measured FPS from time module (includes vsync/frame limiting)
    self.fps = self.alpha * (actual_fps as f64) + (1.0 - self.alpha) * self.fps;
  }
}

/// Helper to time a code block and return the duration.
#[inline]
pub fn time_block<F, R>(f: F) -> (R, Duration)
where
  F: FnOnce() -> R,
{
  let start = Instant::now();
  let result = f();
  (result, start.elapsed())
}

// ============================================================================
// Colors
// ============================================================================

/// Debug visualization colors
pub mod colors {
  /// Bright green for collision boundaries
  pub const COLLIDER: [u8; 4] = [0x00, 0xff, 0x00, 0xff];
  /// Cyan for velocity vectors
  pub const VELOCITY: [u8; 4] = [0x00, 0xff, 0xff, 0xff];
  /// Yellow for origin crosshairs
  pub const ORIGIN: [u8; 4] = [0xff, 0xff, 0x00, 0xff];
  /// Red for error indicators
  pub const ERROR: [u8; 4] = [0xff, 0x00, 0x00, 0xff];
}

// ============================================================================
// Debug Rendering Functions
// ============================================================================

/// Render collision debug visualization.
///
/// Draws outlines around all entities with `Collider` or `GroupedCollider` components.
/// Uses bright green to clearly stand out against game graphics.
pub fn render_colliders(frame: &mut [u8], width: u32, height: u32, world: &World, camera: &Camera) {
  // Render standard Collider components
  for entity in world.entities_with_two::<Transform, Collider>() {
    let Some(transform) = world.get_component::<Transform>(entity) else {
      continue;
    };
    let Some(collider) = world.get_component::<Collider>(entity) else {
      continue;
    };

    // Calculate world position with offset (scaled by transform)
    let world_x = transform.x + collider.offset_x * transform.scale_x;
    let world_y = transform.y + collider.offset_y * transform.scale_y;

    // Convert to screen position
    let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);

    match &collider.shape {
      ColliderShape::Box {
        width: box_w,
        height: box_h,
      } => {
        // Apply transform scale AND camera zoom to collider dimensions
        let scaled_w = (*box_w * transform.scale_x * camera.zoom) as i32;
        let scaled_h = (*box_h * transform.scale_y * camera.zoom) as i32;
        draw_rect_outline(
          frame,
          width,
          height,
          screen_x as i32,
          screen_y as i32,
          scaled_w,
          scaled_h,
          colors::COLLIDER,
        );
      }
      ColliderShape::Circle { radius } => {
        // Apply average scale for circle (circles don't scale non-uniformly well)
        let avg_scale = (transform.scale_x + transform.scale_y) / 2.0;
        let scaled_radius = (*radius * avg_scale * camera.zoom) as i32;
        draw_circle_outline(
          frame,
          width,
          height,
          screen_x as i32,
          screen_y as i32,
          scaled_radius,
          colors::COLLIDER,
        );
      }
    }
  }

  // Render GroupedCollider components (used by composite tiles)
  for entity in world.entities_with_two::<Transform, GroupedCollider>() {
    let Some(transform) = world.get_component::<Transform>(entity) else {
      continue;
    };
    let Some(grouped) = world.get_component::<GroupedCollider>(entity) else {
      continue;
    };

    // Calculate world position with offset
    let world_x = transform.x + grouped.offset_x;
    let world_y = transform.y + grouped.offset_y;

    // Convert to screen position
    let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);

    let scaled_w = (grouped.width * camera.zoom) as i32;
    let scaled_h = (grouped.height * camera.zoom) as i32;
    draw_rect_outline(
      frame,
      width,
      height,
      screen_x as i32,
      screen_y as i32,
      scaled_w,
      scaled_h,
      colors::COLLIDER,
    );
  }
}

/// Render velocity vectors on all entities with `Velocity` components.
///
/// Draws cyan arrows showing the direction and magnitude of movement.
pub fn render_velocities(
  frame: &mut [u8],
  width: u32,
  height: u32,
  world: &World,
  camera: &Camera,
) {
  const VELOCITY_SCALE: f32 = 0.1; // Scale factor for visualizing velocity

  for entity in world.entities_with_two::<Transform, Velocity>() {
    let Some(transform) = world.get_component::<Transform>(entity) else {
      continue;
    };
    let Some(velocity) = world.get_component::<Velocity>(entity) else {
      continue;
    };

    // Skip if not moving
    if velocity.dx.abs() < 0.1 && velocity.dy.abs() < 0.1 {
      continue;
    }

    let (screen_x, screen_y) = camera.world_to_screen(transform.x, transform.y);
    let end_x = screen_x + velocity.dx * VELOCITY_SCALE * camera.zoom;
    let end_y = screen_y + velocity.dy * VELOCITY_SCALE * camera.zoom;

    draw_line(
      frame,
      width,
      height,
      screen_x as i32,
      screen_y as i32,
      end_x as i32,
      end_y as i32,
      colors::VELOCITY,
    );
  }
}

/// Render origin crosshairs on all entities with `Transform` components.
///
/// Draws small yellow crosshairs at each entity's transform position.
pub fn render_origins(frame: &mut [u8], width: u32, height: u32, world: &World, camera: &Camera) {
  const CROSS_SIZE: i32 = 4;

  for entity in world.entities_with::<Transform>() {
    let Some(transform) = world.get_component::<Transform>(entity) else {
      continue;
    };

    let (screen_x, screen_y) = camera.world_to_screen(transform.x, transform.y);
    let sx = screen_x as i32;
    let sy = screen_y as i32;

    // Horizontal bar
    draw_horizontal_line(
      frame,
      width,
      height,
      sx - CROSS_SIZE,
      sy,
      CROSS_SIZE * 2 + 1,
      colors::ORIGIN,
    );
    // Vertical bar
    draw_vertical_line(
      frame,
      width,
      height,
      sx,
      sy - CROSS_SIZE,
      CROSS_SIZE * 2 + 1,
      colors::ORIGIN,
    );
  }
}

/// Render FPS counter in the bottom-left corner.
pub fn render_fps_counter(ui: &mut UiContext, fps: u64, screen_height: u32) {
  let y = screen_height as f32 - 24.0;
  ui.label(
    &format!("FPS: {}", fps),
    4.0,
    y,
    Color::rgb(255, 255, 255),
    Some(Color::new(0, 0, 0, 180)),
  );
}

/// Render frame timing breakdown in the bottom-left corner.
pub fn render_frame_timing(ui: &mut UiContext, timings: &FrameTimings, screen_height: u32) {
  let line_height = 18.0;
  let base_y = screen_height as f32 - (line_height * 5.0) - 8.0;
  let x = 4.0;
  let bg = Some(Color::new(0, 0, 0, 180));
  let white = Color::rgb(255, 255, 255);
  let dim = Color::rgb(180, 180, 180);

  // Note: Update time includes game logic + physics (combined in fixed timestep loop)
  ui.label(
    &format!("Update:  {:5.2}ms", timings.update.as_secs_f64() * 1000.0),
    x,
    base_y,
    white,
    bg,
  );
  ui.label(
    &format!("Render:  {:5.2}ms", timings.render.as_secs_f64() * 1000.0),
    x,
    base_y + line_height,
    white,
    bg,
  );
  ui.label(
    &format!("Present: {:5.2}ms", timings.present.as_secs_f64() * 1000.0),
    x,
    base_y + line_height * 2.0,
    white,
    bg,
  );
  ui.label(
    "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
    x,
    base_y + line_height * 3.0,
    dim,
    bg,
  );
  ui.label(
    &format!(
      "Total:   {:5.2}ms ({} FPS)",
      timings.total.as_secs_f64() * 1000.0,
      timings.fps
    ),
    x,
    base_y + line_height * 4.0,
    white,
    bg,
  );
}

// ============================================================================
// Drawing Primitives
// ============================================================================

/// Draw a rectangle outline (1px thick).
#[allow(clippy::too_many_arguments)]
pub fn draw_rect_outline(
  frame: &mut [u8],
  frame_width: u32,
  frame_height: u32,
  x: i32,
  y: i32,
  w: i32,
  h: i32,
  color: [u8; 4],
) {
  // Top edge
  draw_horizontal_line(frame, frame_width, frame_height, x, y, w, color);
  // Bottom edge
  draw_horizontal_line(frame, frame_width, frame_height, x, y + h - 1, w, color);
  // Left edge
  draw_vertical_line(frame, frame_width, frame_height, x, y, h, color);
  // Right edge
  draw_vertical_line(frame, frame_width, frame_height, x + w - 1, y, h, color);
}

/// Draw a horizontal line with proper bounds checking.
#[inline]
pub fn draw_horizontal_line(
  frame: &mut [u8],
  frame_width: u32,
  frame_height: u32,
  x: i32,
  y: i32,
  length: i32,
  color: [u8; 4],
) {
  if y < 0 || y >= frame_height as i32 {
    return;
  }
  // Calculate the visible portion of the line
  let end_x_signed = x.saturating_add(length);
  if end_x_signed <= 0 || x >= frame_width as i32 {
    return; // Entirely off-screen
  }
  let start_x = x.max(0) as u32;
  let end_x = (end_x_signed as u32).min(frame_width);
  for px in start_x..end_x {
    let idx = ((y as u32 * frame_width + px) * 4) as usize;
    if idx + 3 < frame.len() {
      frame[idx] = color[0];
      frame[idx + 1] = color[1];
      frame[idx + 2] = color[2];
      frame[idx + 3] = color[3];
    }
  }
}

/// Draw a vertical line with proper bounds checking.
#[inline]
pub fn draw_vertical_line(
  frame: &mut [u8],
  frame_width: u32,
  frame_height: u32,
  x: i32,
  y: i32,
  length: i32,
  color: [u8; 4],
) {
  if x < 0 || x >= frame_width as i32 {
    return;
  }
  // Calculate the visible portion of the line
  let end_y_signed = y.saturating_add(length);
  if end_y_signed <= 0 || y >= frame_height as i32 {
    return; // Entirely off-screen
  }
  let start_y = y.max(0) as u32;
  let end_y = (end_y_signed as u32).min(frame_height);
  for py in start_y..end_y {
    let idx = ((py * frame_width + x as u32) * 4) as usize;
    if idx + 3 < frame.len() {
      frame[idx] = color[0];
      frame[idx + 1] = color[1];
      frame[idx + 2] = color[2];
      frame[idx + 3] = color[3];
    }
  }
}

/// Draw a line using Bresenham's algorithm.
#[allow(clippy::too_many_arguments)]
pub fn draw_line(
  frame: &mut [u8],
  width: u32,
  height: u32,
  x0: i32,
  y0: i32,
  x1: i32,
  y1: i32,
  color: [u8; 4],
) {
  let dx = (x1 - x0).abs();
  let dy = -(y1 - y0).abs();
  let sx = if x0 < x1 { 1 } else { -1 };
  let sy = if y0 < y1 { 1 } else { -1 };
  let mut err = dx + dy;

  let mut x = x0;
  let mut y = y0;

  loop {
    set_pixel(frame, width, height, x, y, color);

    if x == x1 && y == y1 {
      break;
    }

    let e2 = 2 * err;
    if e2 >= dy {
      if x == x1 {
        break;
      }
      err += dy;
      x += sx;
    }
    if e2 <= dx {
      if y == y1 {
        break;
      }
      err += dx;
      y += sy;
    }
  }
}

/// Draw a circle outline using midpoint circle algorithm.
pub fn draw_circle_outline(
  frame: &mut [u8],
  frame_width: u32,
  frame_height: u32,
  cx: i32,
  cy: i32,
  radius: i32,
  color: [u8; 4],
) {
  if radius <= 0 {
    return;
  }

  let mut x = radius;
  let mut y = 0;
  let mut err = 0;

  while x >= y {
    // Draw 8 octants
    set_pixel(frame, frame_width, frame_height, cx + x, cy + y, color);
    set_pixel(frame, frame_width, frame_height, cx + y, cy + x, color);
    set_pixel(frame, frame_width, frame_height, cx - y, cy + x, color);
    set_pixel(frame, frame_width, frame_height, cx - x, cy + y, color);
    set_pixel(frame, frame_width, frame_height, cx - x, cy - y, color);
    set_pixel(frame, frame_width, frame_height, cx - y, cy - x, color);
    set_pixel(frame, frame_width, frame_height, cx + y, cy - x, color);
    set_pixel(frame, frame_width, frame_height, cx + x, cy - y, color);

    y += 1;
    err += 1 + 2 * y;
    if 2 * (err - x) + 1 > 0 {
      x -= 1;
      err += 1 - 2 * x;
    }
  }
}

/// Set a single pixel (with bounds checking).
#[inline]
pub fn set_pixel(frame: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
  if x >= 0 && x < width as i32 && y >= 0 && y < height as i32 {
    let idx = ((y as u32 * width + x as u32) * 4) as usize;
    if idx + 3 < frame.len() {
      frame[idx] = color[0];
      frame[idx + 1] = color[1];
      frame[idx + 2] = color[2];
      frame[idx + 3] = color[3];
    }
  }
}

// ============================================================================
// GPU Debug Overlay
// ============================================================================

use crate::core::graphics::{BatchCollector, SpriteGpuRenderer, SpriteInstance};
use crate::core::ui::BitmapFont;

/// Debug overlay texture ID constant
pub const DEBUG_FONT_TEXTURE_ID: &str = "__debug_font__";

/// Z-order for debug overlays (renders on top of everything)
pub const DEBUG_OVERLAY_Z_ORDER: i32 = i32::MAX - 1000;

/// Upload the debug font texture to GPU for overlay rendering.
pub fn upload_debug_font(
  font: &BitmapFont,
  gpu: &mut SpriteGpuRenderer,
  device: &wgpu::Device,
  queue: &wgpu::Queue,
) {
  gpu.upload_texture(
    device,
    queue,
    DEBUG_FONT_TEXTURE_ID,
    font.atlas_width(),
    font.atlas_height(),
    font.atlas_data(),
  );
}

/// Add text to the GPU batch collector for overlay rendering.
///
/// This renders text as GPU sprites, ensuring it appears on top of all game content.
#[allow(clippy::too_many_arguments)]
pub fn add_gpu_text(
  batch: &mut BatchCollector,
  font: &BitmapFont,
  text: &str,
  x: f32,
  y: f32,
  color: Color,
) {
  let char_w = font.char_width() as f32;
  let char_h = font.char_height() as f32;

  let mut cursor_x = x;

  for ch in text.chars() {
    if let Some((u_min, v_min, u_max, v_max)) = font.char_uv(ch) {
      // Position is top-left corner of the glyph (consistent with particles demo)
      let instance = SpriteInstance::new()
        .with_transform(cursor_x, y, 0.0, char_w, char_h)
        .with_uv(u_min, v_min, u_max, v_max)
        .with_tint(color, 1.0);

      batch.add(
        DEBUG_FONT_TEXTURE_ID,
        false,
        instance,
        DEBUG_OVERLAY_Z_ORDER,
      );
    }
    cursor_x += char_w;
  }
}

/// Add a filled rectangle background to the GPU batch.
///
/// Uses a 1x1 white pixel region from the font texture (if available)
/// or just skips if not available.
pub fn add_gpu_rect(
  batch: &mut BatchCollector,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  color: Color,
) {
  // Use a small region of the font texture as a "solid" pixel
  // We'll use the very first pixel which should be background
  // Position is top-left corner
  let instance = SpriteInstance::new()
    .with_transform(x, y, 0.0, width, height)
    .with_uv(0.0, 0.0, 0.001, 0.001) // Tiny UV = single pixel sample
    .with_tint(color, 1.0);

  batch.add(
    DEBUG_FONT_TEXTURE_ID,
    false,
    instance,
    DEBUG_OVERLAY_Z_ORDER - 1,
  ); // Behind text
}

/// Render FPS counter as GPU overlay (bottom-left).
pub fn render_fps_counter_gpu(
  batch: &mut BatchCollector,
  font: &BitmapFont,
  smoothed: &SmoothedFrameTimings,
  _screen_width: f32,
  screen_height: f32,
) {
  let text = format!("FPS: {}", smoothed.fps.round() as u32);
  let char_h = font.char_height() as f32;
  let x = 4.0;
  let y = screen_height - char_h - 8.0;

  // Background
  let text_width = font.text_width(&text) as f32;
  add_gpu_rect(
    batch,
    x - 2.0,
    y - 2.0,
    text_width + 4.0,
    char_h + 4.0,
    Color::new(0, 0, 0, 180),
  );

  // Text
  add_gpu_text(batch, font, &text, x, y, Color::WHITE);
}

/// Render frame timing breakdown as GPU overlay (bottom-left).
pub fn render_frame_timing_gpu(
  batch: &mut BatchCollector,
  font: &BitmapFont,
  smoothed: &SmoothedFrameTimings,
  _screen_width: f32,
  screen_height: f32,
) {
  let char_h = font.char_height() as f32;
  let line_height = char_h + 2.0;
  let x = 4.0;
  let base_y = screen_height - (line_height * 5.0) - 8.0;

  let lines = [
    format!("Update:  {:5.2}ms", smoothed.update_ms),
    format!("Render:  {:5.2}ms", smoothed.render_ms),
    format!("Present: {:5.2}ms", smoothed.present_ms),
    String::from(
      "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
    ),
    format!(
      "Total:   {:5.2}ms ({} FPS)",
      smoothed.total_ms,
      smoothed.fps.round() as u32
    ),
  ];

  // Calculate max width for background
  let max_width = lines.iter().map(|l| font.text_width(l)).max().unwrap_or(0) as f32;

  // Background
  add_gpu_rect(
    batch,
    x - 2.0,
    base_y - 2.0,
    max_width + 4.0,
    line_height * 5.0 + 4.0,
    Color::new(0, 0, 0, 180),
  );

  // Lines
  for (i, line) in lines.iter().enumerate() {
    add_gpu_text(
      batch,
      font,
      line,
      x,
      base_y + (i as f32 * line_height),
      Color::WHITE,
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn debug_config_defaults_to_disabled() {
    let config = DebugConfig::default();
    assert!(!config.show_colliders);
    assert!(!config.show_fps);
    assert!(!config.show_velocities);
    assert!(!config.show_origins);
  }

  #[test]
  fn set_pixel_respects_bounds() {
    let mut frame = vec![0u8; 16]; // 2x2 RGBA
    let width = 2u32;
    let height = 2u32;
    let white = [255, 255, 255, 255];

    // Valid pixel
    set_pixel(&mut frame, width, height, 0, 0, white);
    assert_eq!(&frame[0..4], &white);

    // Out of bounds - should not modify
    set_pixel(&mut frame, width, height, -1, 0, white);
    set_pixel(&mut frame, width, height, 0, -1, white);
    set_pixel(&mut frame, width, height, 2, 0, white);
    set_pixel(&mut frame, width, height, 0, 2, white);
    // Frame should still have only first pixel set
    assert_eq!(&frame[0..4], &white);
    assert_eq!(&frame[4..8], &[0, 0, 0, 0]);
  }

  #[test]
  fn horizontal_line_clips_correctly() {
    let mut frame = vec![0u8; 40]; // 10x1 RGBA
    let white = [255, 255, 255, 255];

    // Line starting off-screen left, ending on-screen
    draw_horizontal_line(&mut frame, 10, 1, -5, 0, 10, white);

    // First 5 pixels should be white (indices 0-4)
    for i in 0..5 {
      let idx = i * 4;
      assert_eq!(&frame[idx..idx + 4], &white, "pixel {} should be white", i);
    }
  }

  #[test]
  fn vertical_line_clips_correctly() {
    let mut frame = vec![0u8; 40]; // 1x10 RGBA
    let white = [255, 255, 255, 255];

    // Line starting off-screen top, ending on-screen
    draw_vertical_line(&mut frame, 1, 10, 0, -5, 10, white);

    // First 5 rows should be white
    for i in 0..5 {
      let idx = i * 4;
      assert_eq!(&frame[idx..idx + 4], &white, "row {} should be white", i);
    }
  }
}
