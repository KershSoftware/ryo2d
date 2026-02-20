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
//! | `enable_profiler` | Open flamegraph profiler window |
//!
//! # Available Info Overlays
//!
//! | Option | Description |
//! |--------|-------------|
//! | `show_fps` | Display FPS counter overlay |
//! | `show_frame_timing` | Display frame timing breakdown |
//! | `show_stats` | Display entity/sprite/batch statistics |
//!
//! # Runtime Debug Key Toggles
//!
//! **Game Debug Keys (F1-F4)** - Available to players unless `disable_debug_keys = true`:
//!
//! | Key | Action |
//! |-----|--------|
//! | F1 | Cycle overlay mode (Off → FPS → Frame Timing → Full Stats → Off) |
//! | F2 | Toggle collision boundaries |
//! | F3 | Toggle velocity vectors |
//! | F4 | Toggle origin points |
//!
//! **Dev Window Keys (F8-F9)** - Dev-only by default, requires `dev_window_keys = true` in release:
//!
//! | Key | Action |
//! |-----|--------|
//! | F8 | Toggle profiler window (requires `profiler` feature) |
//! | F9 | Toggle logger window |
//!
//! Set `disable_debug_keys = true` in `[debug]` to disable F1-F4 toggles.
//! Set `dev_window_keys = true` in `[debug]` to enable F8-F9 in release builds.
//!
//! # Profiling Workflow
//!
//! Frame timing is always captured with negligible overhead. Use the F1 key or
//! `show_frame_timing = true` to display timing breakdown:
//!
//! | Metric | High Value Indicates |
//! |--------|---------------------|
//! | Update | Heavy game logic, too many entities, expensive AI |
//! | Physics | Too many colliders, complex shapes, small timestep |
//! | Render | Too many draw calls, shader complexity, large textures |
//! | Present | GPU-bound, vsync waiting, driver overhead |
//!
//! **Debug build features:**
//! - Scene `on_enter()` timing warnings to console (>5ms)
//! - Slow frame warnings to console (>33ms)
//!
//! # Ryo.toml Configuration
//!
//! ```toml
//! [debug]
//! show_colliders = true
//! show_velocities = false
//! show_origins = false
//! disable_debug_keys = false
//! enable_profiler = true   # Open flamegraph profiler window
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

// ============================================================================
// Overlay Mode
// ============================================================================

/// Debug overlay display mode.
///
/// Controls which debug information is displayed on screen.
/// Cycle through modes with the F1 key (unless `disable_debug_keys` is set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayMode {
  /// No debug overlays shown
  #[default]
  Off,
  /// Show FPS counter only
  FpsOnly,
  /// Show FPS and frame timing breakdown
  FrameTiming,
  /// Show FPS, frame timing, and entity/sprite/batch stats
  FullStats,
}

impl OverlayMode {
  /// Cycle to the next overlay mode.
  ///
  /// Off → FpsOnly → FrameTiming → FullStats → Off
  pub fn next(self) -> Self {
    match self {
      OverlayMode::Off => OverlayMode::FpsOnly,
      OverlayMode::FpsOnly => OverlayMode::FrameTiming,
      OverlayMode::FrameTiming => OverlayMode::FullStats,
      OverlayMode::FullStats => OverlayMode::Off,
    }
  }

  /// Returns the debug config flags for this overlay mode.
  ///
  /// Returns (show_fps, show_frame_timing, show_stats)
  ///
  /// Note: show_fps is false when show_frame_timing is true because
  /// the frame timing overlay already displays FPS in its total line.
  pub fn to_flags(self) -> (bool, bool, bool) {
    match self {
      OverlayMode::Off => (false, false, false),
      OverlayMode::FpsOnly => (true, false, false),
      // Frame timing includes FPS, so no separate FPS counter needed
      OverlayMode::FrameTiming => (false, true, false),
      OverlayMode::FullStats => (false, true, true),
    }
  }

  /// Create overlay mode from debug config flags.
  ///
  /// Takes (show_fps, show_frame_timing, show_stats) and returns the matching mode.
  pub fn from_flags(show_fps: bool, show_frame_timing: bool, show_stats: bool) -> Self {
    match (show_fps, show_frame_timing, show_stats) {
      (_, true, true) => OverlayMode::FullStats,
      (_, true, false) => OverlayMode::FrameTiming,
      (true, false, false) => OverlayMode::FpsOnly,
      _ => OverlayMode::Off,
    }
  }
}

// ============================================================================
// Debug Stats
// ============================================================================

/// Runtime debug statistics for the current frame.
///
/// Captured each frame before batch clearing for display in the stats overlay.
#[derive(Debug, Clone, Default)]
pub struct DebugStats {
  /// Number of active entities in the ECS world
  pub entity_count: u32,
  /// Number of draw calls (GPU batches)
  pub draw_calls: u32,
  /// Number of sprites submitted for rendering
  pub sprite_count: u32,
}

/// Smoothed debug statistics for stable overlay display.
///
/// Uses exponential moving average to prevent jittery numbers.
#[derive(Debug, Clone)]
pub struct SmoothedDebugStats {
  /// Smoothed entity count
  pub entity_count: f64,
  /// Smoothed draw call count
  pub draw_calls: f64,
  /// Smoothed sprite count
  pub sprite_count: f64,
  /// Smoothing factor (0.0 = no change, 1.0 = instant update)
  alpha: f64,
}

impl Default for SmoothedDebugStats {
  fn default() -> Self {
    Self {
      entity_count: 0.0,
      draw_calls: 0.0,
      sprite_count: 0.0,
      alpha: 0.05, // Smooth over ~20 frames
    }
  }
}

impl SmoothedDebugStats {
  /// Update smoothed values from raw stats.
  pub fn update(&mut self, raw: &DebugStats) {
    self.entity_count =
      self.alpha * (raw.entity_count as f64) + (1.0 - self.alpha) * self.entity_count;
    self.draw_calls = self.alpha * (raw.draw_calls as f64) + (1.0 - self.alpha) * self.draw_calls;
    self.sprite_count =
      self.alpha * (raw.sprite_count as f64) + (1.0 - self.alpha) * self.sprite_count;
  }
}

// ============================================================================
// Debug Configuration
// ============================================================================

/// Debug configuration for engine rendering.
///
/// Controls which debug visualizations are enabled.
/// Can be loaded from `Ryo.toml` or set programmatically.
#[derive(Debug, Clone)]
pub struct DebugConfig {
  /// Whether to render collision boundaries (bright green outlines)
  pub show_colliders: bool,
  /// Whether to display FPS counter in corner
  pub show_fps: bool,
  /// Whether to display frame timing breakdown (update/physics/render/present)
  pub show_frame_timing: bool,
  /// Whether to display entity/sprite/batch statistics
  pub show_stats: bool,
  /// Whether to draw velocity vectors on physics entities
  pub show_velocities: bool,
  /// Whether to draw entity origin points (small crosshairs at transform position)
  pub show_origins: bool,
  /// Whether to open the flamegraph profiler window (requires --features profiler or `ryo2d dev --profiler`)
  pub enable_profiler: bool,
  /// Whether to open the logger dev window at startup (shows engine/game logs in real-time)
  pub logger_window: bool,
  /// Whether to enable dev window hotkeys (F8=profiler, F9=logger).
  /// Defaults to true in debug builds, false in release builds.
  /// Set to true in release builds to enable dev tools for testing.
  pub dev_window_keys: bool,
  /// Whether to enable the developer console (toggled with backtick key).
  /// Defaults to true in debug builds, false in release builds.
  pub dev_console: bool,
}

impl Default for DebugConfig {
  fn default() -> Self {
    Self {
      show_colliders: false,
      show_fps: false,
      show_frame_timing: false,
      show_stats: false,
      show_velocities: false,
      show_origins: false,
      enable_profiler: false, // Disabled by default (opt-in)
      logger_window: false,   // Disabled by default (opt-in)
      // Dev window keys (F8/F9) enabled by default in debug builds only
      #[cfg(debug_assertions)]
      dev_window_keys: true,
      #[cfg(not(debug_assertions))]
      dev_window_keys: false,
      // Dev console enabled by default in debug builds only
      #[cfg(debug_assertions)]
      dev_console: true,
      #[cfg(not(debug_assertions))]
      dev_console: false,
    }
  }
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
  /// Total work time (excludes sleep/vsync wait)
  pub total: Duration,
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
  /// Smoothed total work time in milliseconds (excludes sleep/vsync wait)
  pub total_ms: f64,
  /// Smoothed FPS from wall-clock frame-to-frame time (includes sleep/vsync)
  pub fps: f64,
  /// Smoothing factor (0.0 = no change, 1.0 = instant update)
  alpha: f64,
  /// Tracks wall-clock time between update() calls for real FPS measurement
  last_update: Option<Instant>,
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
      last_update: None,
    }
  }
}

impl SmoothedFrameTimings {
  /// Update smoothed values from raw frame timings.
  ///
  /// Work-time fields (update, render, present, total) come from `FrameTimings`.
  /// FPS is measured from wall-clock time between consecutive `update()` calls,
  /// so it includes sleep/vsync wait and reflects actual rendered frames per second.
  pub fn update(&mut self, raw: &FrameTimings) {
    let update_ms = raw.update.as_secs_f64() * 1000.0;
    let render_ms = raw.render.as_secs_f64() * 1000.0;
    let present_ms = raw.present.as_secs_f64() * 1000.0;
    let total_ms = raw.total.as_secs_f64() * 1000.0;

    // Exponential moving average: smoothed = alpha * new + (1 - alpha) * old
    self.update_ms = self.alpha * update_ms + (1.0 - self.alpha) * self.update_ms;
    self.render_ms = self.alpha * render_ms + (1.0 - self.alpha) * self.render_ms;
    self.present_ms = self.alpha * present_ms + (1.0 - self.alpha) * self.present_ms;
    self.total_ms = self.alpha * total_ms + (1.0 - self.alpha) * self.total_ms;

    // FPS from real wall-clock time between frames (includes sleep/vsync)
    let now = Instant::now();
    if let Some(last) = self.last_update {
      let real_dt = now.duration_since(last).as_secs_f64();
      if real_dt > 0.0 {
        let instantaneous_fps = 1.0 / real_dt;
        self.fps = self.alpha * instantaneous_fps + (1.0 - self.alpha) * self.fps;
      }
    }
    self.last_update = Some(now);
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

    let aabb = collider.world_aabb(&transform);
    let (cx, cy) = aabb.center();

    match &collider.shape {
      ColliderShape::Box { .. } => {
        let scaled_w = (aabb.width() * camera.zoom) as i32;
        let scaled_h = (aabb.height() * camera.zoom) as i32;

        let (screen_cx, screen_cy) = camera.world_to_screen(cx, cy);
        let screen_x = screen_cx as i32 - scaled_w / 2;
        let screen_y = screen_cy as i32 - scaled_h / 2;

        draw_rect_outline(
          frame,
          width,
          height,
          screen_x,
          screen_y,
          scaled_w,
          scaled_h,
          colors::COLLIDER,
        );
      }
      ColliderShape::Circle { radius } => {
        let (screen_x, screen_y) = camera.world_to_screen(cx, cy);

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

    // Calculate pivot-aware collider center position
    let pivot_offset_x = (0.5 - transform.pivot_x) * grouped.width;
    let pivot_offset_y = (0.5 - transform.pivot_y) * grouped.height;
    let center_x = transform.x + grouped.offset_x + pivot_offset_x;
    let center_y = transform.y + grouped.offset_y + pivot_offset_y;

    let scaled_w = (grouped.width * camera.zoom) as i32;
    let scaled_h = (grouped.height * camera.zoom) as i32;

    // Convert center to screen, then offset to top-left for drawing
    let (screen_cx, screen_cy) = camera.world_to_screen(center_x, center_y);
    let screen_x = screen_cx as i32 - scaled_w / 2;
    let screen_y = screen_cy as i32 - scaled_h / 2;

    draw_rect_outline(
      frame,
      width,
      height,
      screen_x,
      screen_y,
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

/// Height of the frame timing overlay (5 lines + padding).
pub const FRAME_TIMING_HEIGHT: f32 = 18.0 * 5.0 + 8.0;

/// Render FPS counter in the bottom-left corner.
///
/// When `frame_timing_below` is true, positions the FPS counter above the frame timing overlay.
pub fn render_fps_counter(
  ui: &mut UiContext,
  fps: u64,
  screen_height: u32,
  frame_timing_below: bool,
) {
  let offset = if frame_timing_below {
    FRAME_TIMING_HEIGHT
  } else {
    0.0
  };
  let y = screen_height as f32 - 24.0 - offset;
  ui.label(
    &format!("FPS: {}", fps),
    4.0,
    y,
    Color::rgb(255, 255, 255),
    Some(Color::new(0, 0, 0, 180)),
  );
}

/// Render frame timing breakdown in the bottom-left corner.
///
/// Uses smoothed timings from previous frames for stable display.
/// Raw timings would show incomplete data since this is called mid-frame.
pub fn render_frame_timing(
  ui: &mut UiContext,
  smoothed: &SmoothedFrameTimings,
  screen_height: u32,
) {
  let line_height = 18.0;
  let base_y = screen_height as f32 - (line_height * 5.0) - 8.0;
  let x = 4.0;
  let bg = Some(Color::new(0, 0, 0, 180));
  let white = Color::rgb(255, 255, 255);
  let dim = Color::rgb(180, 180, 180);

  // Note: Update time includes game logic + physics (combined in fixed timestep loop)
  ui.label(
    &format!("Update:  {:5.2}ms", smoothed.update_ms),
    x,
    base_y,
    white,
    bg,
  );
  ui.label(
    &format!("Render:  {:5.2}ms", smoothed.render_ms),
    x,
    base_y + line_height,
    white,
    bg,
  );
  ui.label(
    &format!("Present: {:5.2}ms", smoothed.present_ms),
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
      smoothed.total_ms,
      smoothed.fps.round() as u32
    ),
    x,
    base_y + line_height * 4.0,
    white,
    bg,
  );
}

/// Height of the FPS counter overlay (1 line + padding).
pub const FPS_COUNTER_HEIGHT: f32 = 24.0;

/// Height of the stats overlay (4 lines + padding).
pub const STATS_HEIGHT: f32 = 18.0 * 4.0 + 8.0;

/// Render debug stats (entity/sprite/batch counts) in the bottom-left corner.
///
/// Positioned above the frame timing overlay (FPS counter is not shown when stats are visible).
pub fn render_stats(ui: &mut UiContext, stats: &SmoothedDebugStats, screen_height: u32) {
  let line_height = 18.0;
  // Position above frame timing only (FPS counter not shown when frame timing is visible)
  let base_y = screen_height as f32 - FRAME_TIMING_HEIGHT - (line_height * 4.0) - 8.0;
  let x = 4.0;
  let bg = Some(Color::new(0, 0, 0, 180));
  let white = Color::rgb(255, 255, 255);
  let dim = Color::rgb(180, 180, 180);

  ui.label(
    &format!("Entities: {:5}", stats.entity_count.round() as u32),
    x,
    base_y,
    white,
    bg,
  );
  ui.label(
    &format!("Sprites:  {:5}", stats.sprite_count.round() as u32),
    x,
    base_y + line_height,
    white,
    bg,
  );
  ui.label(
    &format!("Batches:  {:5}", stats.draw_calls.round() as u32),
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
///
/// When `frame_timing_below` is true, positions the FPS counter above the frame timing overlay.
pub fn render_fps_counter_gpu(
  batch: &mut BatchCollector,
  font: &BitmapFont,
  smoothed: &SmoothedFrameTimings,
  _screen_width: f32,
  screen_height: f32,
  frame_timing_below: bool,
) {
  let text = format!("FPS: {}", smoothed.fps.round() as u32);
  let char_h = font.char_height() as f32;
  let line_height = char_h + 2.0;
  let x = 4.0;

  // Calculate offset if frame timing is below
  let offset = if frame_timing_below {
    line_height * 5.0 + 8.0 // Height of frame timing overlay
  } else {
    0.0
  };
  let y = screen_height - char_h - 8.0 - offset;

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

/// Render debug stats as GPU overlay (bottom-left, above frame timing).
pub fn render_stats_gpu(
  batch: &mut BatchCollector,
  font: &BitmapFont,
  stats: &SmoothedDebugStats,
  _screen_width: f32,
  screen_height: f32,
) {
  let char_h = font.char_height() as f32;
  let line_height = char_h + 2.0;
  let x = 4.0;

  // Position above frame timing only (FPS counter not shown when frame timing is visible)
  let frame_timing_height = line_height * 5.0 + 8.0;
  let base_y = screen_height - frame_timing_height - (line_height * 4.0) - 8.0;

  let lines = [
    format!("Entities: {:5}", stats.entity_count.round() as u32),
    format!("Sprites:  {:5}", stats.sprite_count.round() as u32),
    format!("Batches:  {:5}", stats.draw_calls.round() as u32),
    String::from(
      "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
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
    line_height * 4.0 + 4.0,
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
  fn debug_config_defaults() {
    let config = DebugConfig::default();
    assert!(!config.show_colliders);
    assert!(!config.show_fps);
    assert!(!config.show_frame_timing);
    assert!(!config.show_stats);
    assert!(!config.show_velocities);
    assert!(!config.show_origins);
    // enable_profiler defaults to false (opt-in)
    assert!(!config.enable_profiler);
  }

  #[test]
  fn overlay_mode_cycles_correctly() {
    assert_eq!(OverlayMode::Off.next(), OverlayMode::FpsOnly);
    assert_eq!(OverlayMode::FpsOnly.next(), OverlayMode::FrameTiming);
    assert_eq!(OverlayMode::FrameTiming.next(), OverlayMode::FullStats);
    assert_eq!(OverlayMode::FullStats.next(), OverlayMode::Off);
  }

  #[test]
  fn overlay_mode_to_flags() {
    assert_eq!(OverlayMode::Off.to_flags(), (false, false, false));
    assert_eq!(OverlayMode::FpsOnly.to_flags(), (true, false, false));
    // Frame timing includes FPS in its display, so show_fps is false
    assert_eq!(OverlayMode::FrameTiming.to_flags(), (false, true, false));
    assert_eq!(OverlayMode::FullStats.to_flags(), (false, true, true));
  }

  #[test]
  fn overlay_mode_from_flags() {
    assert_eq!(
      OverlayMode::from_flags(false, false, false),
      OverlayMode::Off
    );
    assert_eq!(
      OverlayMode::from_flags(true, false, false),
      OverlayMode::FpsOnly
    );
    // Frame timing mode regardless of show_fps value
    assert_eq!(
      OverlayMode::from_flags(false, true, false),
      OverlayMode::FrameTiming
    );
    assert_eq!(
      OverlayMode::from_flags(true, true, false),
      OverlayMode::FrameTiming
    );
    // Full stats mode regardless of show_fps value
    assert_eq!(
      OverlayMode::from_flags(false, true, true),
      OverlayMode::FullStats
    );
    assert_eq!(
      OverlayMode::from_flags(true, true, true),
      OverlayMode::FullStats
    );
  }

  #[test]
  fn overlay_mode_default_is_off() {
    assert_eq!(OverlayMode::default(), OverlayMode::Off);
  }

  #[test]
  fn debug_stats_default() {
    let stats = DebugStats::default();
    assert_eq!(stats.entity_count, 0);
    assert_eq!(stats.draw_calls, 0);
    assert_eq!(stats.sprite_count, 0);
  }

  #[test]
  fn smoothed_debug_stats_update() {
    let mut smoothed = SmoothedDebugStats::default();
    let raw = DebugStats {
      entity_count: 100,
      draw_calls: 10,
      sprite_count: 500,
    };

    // After first update, values should move toward raw
    smoothed.update(&raw);
    assert!(smoothed.entity_count > 0.0);
    assert!(smoothed.draw_calls > 0.0);
    assert!(smoothed.sprite_count > 0.0);

    // After many updates, should converge (alpha=0.05 needs ~200 iterations)
    for _ in 0..200 {
      smoothed.update(&raw);
    }
    assert!((smoothed.entity_count - 100.0).abs() < 1.0);
    assert!((smoothed.draw_calls - 10.0).abs() < 1.0);
    assert!((smoothed.sprite_count - 500.0).abs() < 1.0);
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
