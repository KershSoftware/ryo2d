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
//! | `show_fps` | Display FPS counter overlay |
//! | `show_velocities` | Draw velocity vectors on physics entities |
//! | `show_origins` | Draw entity origin points (transform position) |
//!
//! # Ryo.toml Configuration
//!
//! ```toml
//! [debug]
//! show_colliders = true
//! show_fps = true
//! show_velocities = false
//! show_origins = false
//! ```
//!
//! # Programmatic Configuration
//!
//! ```rust,ignore
//! EngineBuilder::new()
//!     .show_colliders(true)
//!     .show_fps(true)
//!     .with_workspace_debug()  // Load from Ryo.toml
//!     .build()
//! ```

use super::camera::Camera;
use super::collision::{Collider, ColliderShape};
use super::ecs::{Transform, Velocity, World};
use super::ui::{Color, UiContext};

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
  /// Whether to draw velocity vectors on physics entities
  pub show_velocities: bool,
  /// Whether to draw entity origin points (small crosshairs at transform position)
  pub show_origins: bool,
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
/// Draws outlines around all entities with `Collider` components.
/// Uses bright green to clearly stand out against game graphics.
pub fn render_colliders(frame: &mut [u8], width: u32, height: u32, world: &World, camera: &Camera) {
  for entity in world.entities_with_two::<Transform, Collider>() {
    let Some(transform) = world.get_component::<Transform>(entity) else {
      continue;
    };
    let Some(collider) = world.get_component::<Collider>(entity) else {
      continue;
    };

    // Calculate world position with offset
    let world_x = transform.x + collider.offset_x;
    let world_y = transform.y + collider.offset_y;

    // Convert to screen position
    let (screen_x, screen_y) = camera.world_to_screen(world_x, world_y);

    match &collider.shape {
      ColliderShape::Box {
        width: box_w,
        height: box_h,
      } => {
        let scaled_w = (*box_w * camera.zoom) as i32;
        let scaled_h = (*box_h * camera.zoom) as i32;
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
        let scaled_radius = (*radius * camera.zoom) as i32;
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

/// Render FPS counter in the top-left corner.
pub fn render_fps_counter(ui: &mut UiContext, fps: u64) {
  ui.label(
    &format!("FPS: {}", fps),
    4.0,
    4.0,
    Color::rgb(255, 255, 255),
    Some(Color::new(0, 0, 0, 180)),
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
