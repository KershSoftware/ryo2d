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

use crate::core::ecs::{GlobalTransform, Vec2, Velocity, Without, World};
use crate::core::graphics::camera_mut::main_view;
use crate::core::graphics::{Camera2d, GroupedCollider};
use crate::core::physics::{Collider, ColliderShape};
use crate::core::render::DebugDraw;
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

/// Half the length of an origin crosshair arm, in screen pixels.
const CROSS_SIZE: f32 = 4.0;

/// Which debug overlays [`overlay_shapes`] draws.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OverlayFlags {
  pub(crate) colliders: bool,
  pub(crate) velocities: bool,
  pub(crate) origins: bool,
}

/// Draw the enabled debug overlays into `debug` for this frame: collider
/// outlines where physics puts them, velocity arrows from the origin and
/// origin crosses (sized in the main camera's pixels).
pub(crate) fn overlay_shapes(world: &mut World, flags: OverlayFlags, debug: &mut DebugDraw) {
  let color = |c: [u8; 4]| Color::new(c[0], c[1], c[2], c[3]);
  if flags.colliders {
    for (global, collider) in world.query::<(&GlobalTransform, &Collider)>().iter(world) {
      let center = collider.world_center(global.translation());
      match collider.shape {
        ColliderShape::Box { width, height } => {
          debug.rect(center, Vec2::new(width, height), color(colors::COLLIDER));
        }
        ColliderShape::Circle { radius } => debug.circle(center, radius, color(colors::COLLIDER)),
      }
    }
    // A grouped collider spans `offset` to `offset + size` from its origin
    for (global, grouped) in world
      .query::<(&GlobalTransform, &GroupedCollider)>()
      .iter(world)
    {
      let size = Vec2::new(grouped.width, grouped.height);
      let center =
        global.translation() + Vec2::new(grouped.offset_x, grouped.offset_y) + size / 2.0;
      debug.rect(center, size, color(colors::COLLIDER));
    }
  }

  if flags.velocities {
    // Scale factor for visualizing velocity
    const VELOCITY_SCALE: f32 = 0.1;
    for (global, velocity) in world.query::<(&GlobalTransform, &Velocity)>().iter(world) {
      if velocity.dx.abs() < 0.1 && velocity.dy.abs() < 0.1 {
        continue;
      }
      let origin = global.translation();
      let end = origin + Vec2::new(velocity.dx, velocity.dy) * VELOCITY_SCALE;
      debug.arrow(origin, end, color(colors::VELOCITY));
    }
  }

  if flags.origins {
    // `CROSS_SIZE` main-camera pixels either side of the origin's pixel
    let scale = main_view(world).scale;
    let pixel = if scale.is_finite() && scale > 0.0 {
      1.0 / scale
    } else {
      1.0
    };
    let size = (2.0 * CROSS_SIZE + 1.0) * pixel;
    // Cameras are not drawn
    for global in world
      .query_filtered::<&GlobalTransform, Without<Camera2d>>()
      .iter(world)
    {
      debug.cross(global.translation(), size, color(colors::ORIGIN));
    }
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn collider_outlines_are_drawn_where_the_collider_is() {
    use crate::core::ecs::Transform;
    let mut world = World::new();
    world.spawn((
      Transform::from_xy(100.0, 50.0),
      Collider::box_collider(20.0, 10.0),
    ));
    let colliders = OverlayFlags {
      colliders: true,
      ..Default::default()
    };
    let mut debug = DebugDraw::default();
    overlay_shapes(&mut world, colliders, &mut debug);
    let shapes: Vec<_> = debug
      .each()
      .map(|(shape, points)| (shape.kind, points.to_vec()))
      .collect();
    assert_eq!(
      shapes,
      [(
        crate::core::render::ShapeKind::Rect,
        vec![Vec2::new(100.0, 50.0), Vec2::new(20.0, 10.0)]
      )]
    );
    debug.end_frame();
    overlay_shapes(&mut world, OverlayFlags::default(), &mut debug);
    assert_eq!(debug.len(), 0);
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
