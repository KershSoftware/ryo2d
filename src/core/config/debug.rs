//! Debug visualization and rendering utilities
//!
//! This module provides engine-level debug visualization for development.
//! All debug features can be configured via `Ryo.toml` or the `EngineBuilder`.
//!
//! # Available Debug Options
//!
//! | Option | Description |
//! |--------|-------------|
//! | `show_colliders` | Draw the simulation's colliders, joints and contacts, coloured by body kind |
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
use crate::core::graphics::Camera2d;
use crate::core::graphics::camera_mut::main_view;
use crate::core::physics::Physics;
use crate::core::render::DebugDraw;
use crate::core::text::{FontRole, TextStyle};
use crate::core::ui::{Color, Rect, UiContext};

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
  /// Green for dynamic bodies' colliders
  pub const DYNAMIC: [u8; 4] = [0x00, 0xff, 0x00, 0xff];
  /// Orange for kinematic bodies' colliders
  pub const KINEMATIC: [u8; 4] = [0xff, 0xa0, 0x00, 0xff];
  /// Blue for fixed bodies and bare colliders
  pub const FIXED: [u8; 4] = [0x60, 0x90, 0xff, 0xff];
  /// Yellow for triggers
  pub const TRIGGER: [u8; 4] = [0xff, 0xff, 0x00, 0xff];
  /// Magenta for joints
  pub const JOINT: [u8; 4] = [0xff, 0x00, 0xff, 0xff];
  /// Red for contact points and normals
  pub const CONTACT: [u8; 4] = [0xff, 0x30, 0x30, 0xff];
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
  if flags.colliders
    && let Some(physics) = world.get_resource::<Physics>()
  {
    crate::core::physics::debug::draw(physics, debug);
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

/// The overlays' text: JetBrains Mono, so `{:5}`-padded columns line up
fn overlay_style() -> TextStyle {
  TextStyle::new().role(FontRole::Mono).size(12.0)
}

/// Space below an overlay's rows
const OVERLAY_MARGIN: f32 = 8.0;

/// Distance between overlay rows: a line of text plus the label padding
/// above and below, so the panels touch instead of overlapping
fn row_pitch(ui: &UiContext) -> f32 {
  ui.line_height(&overlay_style()) + 8.0
}

/// Height of the frame timing overlay (5 rows + margin)
fn frame_timing_height(ui: &UiContext) -> f32 {
  row_pitch(ui) * 5.0 + OVERLAY_MARGIN
}

/// A mono label on a translucent panel padded to fit the measured text
fn overlay_label(ui: &mut UiContext, text: &str, x: f32, y: f32, color: Color) {
  let style = overlay_style();
  let size = ui.measure_text(text, &style);
  let padding = 4.0;
  ui.draw_rect(
    Rect::new(
      x - padding,
      y - padding,
      size.x + padding * 2.0,
      size.y + padding * 2.0,
    ),
    Color::new(0, 0, 0, 180),
  );
  ui.text(text).style(&style.color(color)).at(x, y).draw();
}

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
    frame_timing_height(ui)
  } else {
    0.0
  };
  let y = screen_height as f32 - row_pitch(ui) - offset;
  overlay_label(
    ui,
    &format!("FPS: {}", fps),
    4.0,
    y,
    Color::rgb(255, 255, 255),
  );
}

/// The separator row under the overlays' columns
const SEPARATOR: &str = "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}";

/// Render frame timing breakdown in the bottom-left corner.
///
/// Uses smoothed timings from previous frames for stable display.
/// Raw timings would show incomplete data since this is called mid-frame.
pub fn render_frame_timing(
  ui: &mut UiContext,
  smoothed: &SmoothedFrameTimings,
  screen_height: u32,
) {
  let pitch = row_pitch(ui);
  let base_y = screen_height as f32 - frame_timing_height(ui);
  let x = 4.0;
  let white = Color::rgb(255, 255, 255);
  let dim = Color::rgb(180, 180, 180);

  // Note: Update time includes game logic + physics (combined in fixed timestep loop)
  let rows = [
    (format!("Update:  {:5.2}ms", smoothed.update_ms), white),
    (format!("Render:  {:5.2}ms", smoothed.render_ms), white),
    (format!("Present: {:5.2}ms", smoothed.present_ms), white),
    (SEPARATOR.to_string(), dim),
    (
      format!(
        "Total:   {:5.2}ms ({} FPS)",
        smoothed.total_ms,
        smoothed.fps.round() as u32
      ),
      white,
    ),
  ];
  for (i, (text, color)) in rows.iter().enumerate() {
    overlay_label(ui, text, x, base_y + pitch * i as f32, *color);
  }
}

/// Render debug stats (entity/sprite/batch counts) in the bottom-left corner.
///
/// Positioned above the frame timing overlay (FPS counter is not shown when stats are visible).
pub fn render_stats(ui: &mut UiContext, stats: &SmoothedDebugStats, screen_height: u32) {
  let pitch = row_pitch(ui);
  // Position above frame timing only (FPS counter not shown when frame timing is visible)
  let base_y = screen_height as f32 - frame_timing_height(ui) - pitch * 4.0 - OVERLAY_MARGIN;
  let x = 4.0;
  let white = Color::rgb(255, 255, 255);
  let dim = Color::rgb(180, 180, 180);

  let rows = [
    (
      format!("Entities: {:5}", stats.entity_count.round() as u32),
      white,
    ),
    (
      format!("Sprites:  {:5}", stats.sprite_count.round() as u32),
      white,
    ),
    (
      format!("Batches:  {:5}", stats.draw_calls.round() as u32),
      white,
    ),
    (SEPARATOR.to_string(), dim),
  ];
  for (i, (text, color)) in rows.iter().enumerate() {
    overlay_label(ui, text, x, base_y + pitch * i as f32, *color);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(feature = "default-fonts")]
  #[test]
  fn overlay_rows_are_monospace_and_clear_of_each_other() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let (mut commands, mut text) = (
      UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
    let timings = SmoothedFrameTimings::default();
    render_frame_timing(&mut ui, &timings, 720);
    drop(ui);
    let panels: Vec<Rect> = commands
      .iter()
      .filter_map(|c| match c {
        UiCommand::Rect { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect();
    assert_eq!(panels.len(), 5);
    // "Update: ", "Render: " and "Present:" columns have the same length
    assert!(
      (panels[0].width - panels[1].width).abs() < 0.01,
      "{panels:?}"
    );
    assert!(
      (panels[1].width - panels[2].width).abs() < 0.01,
      "{panels:?}"
    );
    for pair in panels.windows(2) {
      assert!(
        pair[0].y + pair[0].height <= pair[1].y + 0.01,
        "rows overlap: {pair:?}"
      );
    }
    let last = panels[4];
    assert!(last.y + last.height <= 720.0, "on screen");
  }

  #[test]
  fn collider_outlines_are_drawn_where_physics_has_them() {
    use crate::core::ecs::Transform;
    use crate::core::physics::Collider;
    use crate::core::physics::test_util::{run, world};
    let (mut world, mut s) = world(60);
    world.spawn((
      Transform::from_xy(100.0, 50.0)
        .with_rotation(std::f32::consts::FRAC_PI_2)
        .with_scale(Vec2::new(2.0, 3.0)),
      Collider::rect(20.0, 10.0),
    ));
    run(&mut world, &mut s, 1);
    let colliders = OverlayFlags {
      colliders: true,
      ..Default::default()
    };
    let mut debug = DebugDraw::default();
    overlay_shapes(&mut world, colliders, &mut debug);
    let points: Vec<Vec2> = debug
      .each()
      .flat_map(|(_, points)| points.to_vec())
      .collect();
    assert!(!points.is_empty());
    let min = points.iter().fold(Vec2::MAX, |m, p| m.min(*p));
    let max = points.iter().fold(Vec2::MIN, |m, p| m.max(*p));
    // Scaled to 40 by 30, then turned a quarter: 30 wide and 40 high
    // around (100, 50)
    assert!(
      (min - Vec2::new(85.0, 30.0)).abs().max_element() < 0.01,
      "{min}"
    );
    assert!(
      (max - Vec2::new(115.0, 70.0)).abs().max_element() < 0.01,
      "{max}"
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
