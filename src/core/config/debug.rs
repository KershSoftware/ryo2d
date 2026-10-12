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
//! **Game Debug Keys (F1-F5)** - Available to players unless `disable_debug_keys = true`:
//!
//! | Key | Action |
//! |-----|--------|
//! | F1 | Cycle overlay mode (Off → FPS → Frame Timing → Full Stats → Off) |
//! | F2 | Toggle collision boundaries |
//! | F3 | Toggle velocity vectors |
//! | F4 | Toggle origin points |
//! | F5 | Toggle borderless fullscreen |
//!
//! **Dev Keys (F9, F10, F12)** - Dev-only by default, requires `dev_window_keys = true` in release:
//!
//! | Key | Action |
//! |-----|--------|
//! | F9 | Toggle logger window |
//! | F10 | Toggle profiler window (requires `profiler` feature) |
//! | F12 | Capture the next frame to `captures/` (or `RYO_CAPTURE_DIR`) |
//!
//! Set `disable_debug_keys = true` in `[debug]` to disable the F1-F5 toggles.
//! Set `dev_window_keys = true` in `[debug]` to enable F9, F10 and F12 in release builds.
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
//! [system]
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

use crate::core::ecs::{GlobalTransform, Vec2, Velocity, World};
use crate::core::graphics::{Camera, GroupedCollider};
use crate::core::physics::{Collider, ColliderShape};
use crate::core::trail::{Line2D, TrailStyle};
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
  /// Number of draw calls issued by the `world` and `unlit_world` passes
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
  /// Whether to enable dev hotkeys (F9=logger, F10=profiler, F12=frame capture).
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
      // Dev window keys (F9/F10/F12) enabled by default in debug builds only
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

use std::time::Duration;
use web_time::Instant;

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

/// A debug shape in whole screen pixels, the unit both the pixel-buffer
/// overlays and the GPU world path's lines are built from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum OverlayShape {
  /// A one-pixel rectangle outline
  Rect { x: i32, y: i32, w: i32, h: i32 },
  /// A one-pixel circle outline
  Circle { x: i32, y: i32, radius: i32 },
  /// A one-pixel line from the first pixel to the second
  Line { x0: i32, y0: i32, x1: i32, y1: i32 },
  /// Origin crosshair arms (a horizontal and a vertical bar) centred here
  Cross { x: i32, y: i32 },
}

/// Half the length of an origin crosshair arm, in pixels.
const CROSS_SIZE: i32 = 4;

/// The collider outlines, `Collider` and `GroupedCollider` components both,
/// drawn where physics simulates them.
fn collider_shapes(world: &mut World, camera: &Camera) -> Vec<(OverlayShape, [u8; 4])> {
  let mut shapes = Vec::new();
  let rect = |center: Vec2, size: Vec2| {
    let (w, h) = ((size.x * camera.zoom) as i32, (size.y * camera.zoom) as i32);
    let (sx, sy) = camera.world_to_screen(center.x, center.y);
    OverlayShape::Rect {
      x: sx as i32 - w / 2,
      y: sy as i32 - h / 2,
      w,
      h,
    }
  };

  for (global, collider) in world.query::<(&GlobalTransform, &Collider)>().iter(world) {
    let center = collider.world_center(global.translation());
    let shape = match collider.shape {
      ColliderShape::Box { width, height } => rect(center, Vec2::new(width, height)),
      ColliderShape::Circle { radius } => {
        let (sx, sy) = camera.world_to_screen(center.x, center.y);
        OverlayShape::Circle {
          x: sx as i32,
          y: sy as i32,
          radius: (radius * camera.zoom) as i32,
        }
      }
    };
    shapes.push((shape, colors::COLLIDER));
  }

  // A grouped collider spans `offset` to `offset + size` from its origin
  for (global, grouped) in world
    .query::<(&GlobalTransform, &GroupedCollider)>()
    .iter(world)
  {
    let size = Vec2::new(grouped.width, grouped.height);
    let center = global.translation() + Vec2::new(grouped.offset_x, grouped.offset_y) + size / 2.0;
    shapes.push((rect(center, size), colors::COLLIDER));
  }
  shapes
}

/// The velocity vectors of moving entities.
fn velocity_shapes(world: &mut World, camera: &Camera) -> Vec<(OverlayShape, [u8; 4])> {
  const VELOCITY_SCALE: f32 = 0.1; // Scale factor for visualizing velocity

  let mut shapes = Vec::new();
  for (global, velocity) in world.query::<(&GlobalTransform, &Velocity)>().iter(world) {
    if velocity.dx.abs() < 0.1 && velocity.dy.abs() < 0.1 {
      continue;
    }
    let origin = global.translation();
    let (screen_x, screen_y) = camera.world_to_screen(origin.x, origin.y);
    let end_x = screen_x + velocity.dx * VELOCITY_SCALE * camera.zoom;
    let end_y = screen_y + velocity.dy * VELOCITY_SCALE * camera.zoom;
    shapes.push((
      OverlayShape::Line {
        x0: screen_x as i32,
        y0: screen_y as i32,
        x1: end_x as i32,
        y1: end_y as i32,
      },
      colors::VELOCITY,
    ));
  }
  shapes
}

/// The origin crosshairs of every placed entity.
fn origin_shapes(world: &mut World, camera: &Camera) -> Vec<(OverlayShape, [u8; 4])> {
  world
    .query::<&GlobalTransform>()
    .iter(world)
    .map(|global| {
      let origin = global.translation();
      let (screen_x, screen_y) = camera.world_to_screen(origin.x, origin.y);
      (
        OverlayShape::Cross {
          x: screen_x as i32,
          y: screen_y as i32,
        },
        colors::ORIGIN,
      )
    })
    .collect()
}

/// Which debug overlays [`overlay_lines`] builds.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OverlayFlags {
  pub(crate) colliders: bool,
  pub(crate) velocities: bool,
  pub(crate) origins: bool,
}

/// The enabled debug overlays as unlit, one-pixel lines on top of everything,
/// for the world passes. The lines use the `Soft` style: a
/// pixel-wide `Solid` line is anti-aliased into a faint smear.
pub(crate) fn overlay_lines(
  world: &mut World,
  camera: &Camera,
  flags: OverlayFlags,
) -> Vec<Line2D> {
  let mut shapes = Vec::new();
  if flags.colliders {
    shapes.extend(collider_shapes(world, camera));
  }
  if flags.velocities {
    shapes.extend(velocity_shapes(world, camera));
  }
  if flags.origins {
    shapes.extend(origin_shapes(world, camera));
  }

  // Lines are in world units: a pixel is 1 / zoom wide
  let width = 1.0 / camera.zoom;
  let point = |x: f32, y: f32| {
    let (wx, wy) = camera.screen_to_world(x, y);
    Vec2::new(wx, wy)
  };
  let line = |points: Vec<Vec2>, color: [u8; 4]| {
    Line2D::new(
      points,
      Color::new(color[0], color[1], color[2], color[3]),
      width,
    )
    .with_style(TrailStyle::Soft)
    .with_lighting(false)
    .with_z_order(i32::MAX)
  };

  let mut lines = Vec::new();
  for (shape, color) in shapes {
    match shape {
      // Each edge is the run of pixels the pixel-buffer outline sets
      OverlayShape::Rect { x, y, w, h } => {
        let (x0, y0, x1, y1) = (x as f32, y as f32, (x + w) as f32, (y + h) as f32);
        for (a, b) in [
          ((x0, y0 + 0.5), (x1, y0 + 0.5)),
          ((x0, y1 - 0.5), (x1, y1 - 0.5)),
          ((x0 + 0.5, y0), (x0 + 0.5, y1)),
          ((x1 - 0.5, y0), (x1 - 0.5, y1)),
        ] {
          lines.push(line(vec![point(a.0, a.1), point(b.0, b.1)], color));
        }
      }
      OverlayShape::Circle { x, y, radius } => {
        if radius > 0 {
          let center = point(x as f32 + 0.5, y as f32 + 0.5);
          let radius = radius as f32 / camera.zoom;
          let mut circle = Line2D::circle(center, radius, Color::WHITE, width, 32);
          circle.set_color(Color::new(color[0], color[1], color[2], color[3]));
          lines.push(
            circle
              .with_style(TrailStyle::Soft)
              .with_lighting(false)
              .with_z_order(i32::MAX),
          );
        }
      }
      OverlayShape::Line { x0, y0, x1, y1 } => {
        let a = point(x0 as f32 + 0.5, y0 as f32 + 0.5);
        let b = point(x1 as f32 + 0.5, y1 as f32 + 0.5);
        lines.push(line(vec![a, b], color));
      }
      OverlayShape::Cross { x, y } => {
        let (cx, cy) = (x as f32, y as f32);
        let arm = CROSS_SIZE as f32;
        let (h0, h1) = (point(cx - arm, cy + 0.5), point(cx + arm + 1.0, cy + 0.5));
        let (v0, v1) = (point(cx + 0.5, cy - arm), point(cx + 0.5, cy + arm + 1.0));
        lines.push(line(vec![h0, h1], color));
        lines.push(line(vec![v0, v1], color));
      }
    }
  }
  lines
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn overlay_lines_outline_colliders_where_physics_puts_them() {
    use crate::core::ecs::{GlobalTransform, Transform};
    let mut world = World::new();
    let t = Transform::from_xy(100.0, 50.0);
    world.spawn((
      t,
      GlobalTransform::from_transform(&t),
      Collider::box_collider(20.0, 10.0),
    ));
    let camera = Camera::centered_at(400.0, 300.0, 800, 600);
    let colliders = OverlayFlags {
      colliders: true,
      ..Default::default()
    };
    let lines = overlay_lines(&mut world, &camera, colliders);
    assert_eq!(lines.len(), 4, "a box is four edges");
    let xs: Vec<f32> = lines
      .iter()
      .flat_map(|line| line.points().iter().map(|p| p.x))
      .collect();
    let min = xs.iter().copied().fold(f32::MAX, f32::min);
    let max = xs.iter().copied().fold(f32::MIN, f32::max);
    assert!(
      (min - 90.0).abs() <= 1.0 && (max - 110.0).abs() <= 1.0,
      "{min}..{max}"
    );
    assert!(overlay_lines(&mut world, &camera, OverlayFlags::default()).is_empty());
  }

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
}
