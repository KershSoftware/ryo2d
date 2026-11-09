//! The overlays the engine draws: modes, statistics, collider and
//! velocity shapes, and the text rows.

use super::timing::SmoothedFrameTimings;
use crate::core::ecs::{GlobalTransform, Vec2, Velocity, Without, World};
use crate::core::graphics::Camera2d;
use crate::core::graphics::camera_mut::main_view;
use crate::core::physics::Physics;
use crate::core::render::DebugDraw;
use crate::core::text::{FontRole, TextStyle};
use crate::core::ui::{Color, Rect, UiContext};

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

/// Runtime debug statistics for the current frame.
///
/// Captured each frame before batch clearing for display in the stats overlay.
#[derive(Debug, Clone, Default)]
pub struct DebugStats {
  /// Number of active entities in the ECS world
  pub entity_count: u32,
  /// Number of draw calls issued by the `world`, `unlit_world`, `lighting`
  /// and `ui` passes
  pub draw_calls: u32,
  /// Number of sprites submitted for rendering (the engine's particles
  /// among them)
  pub sprite_count: u32,
  /// Live particles over every emitter
  pub particles: u32,
  /// The most live particles allowed at once (`EngineBuilder::particle_budget`)
  pub particle_budget: u32,
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
  /// Smoothed live particle count
  pub particles: f64,
  /// The particle budget, as the last frame had it (a setting, not
  /// smoothed: it would climb from 0 at start)
  pub particle_budget: u32,
  /// Smoothing factor (0.0 = no change, 1.0 = instant update)
  alpha: f64,
}

impl Default for SmoothedDebugStats {
  fn default() -> Self {
    Self {
      entity_count: 0.0,
      draw_calls: 0.0,
      sprite_count: 0.0,
      particles: 0.0,
      particle_budget: 0,
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
    self.particles = self.alpha * (raw.particles as f64) + (1.0 - self.alpha) * self.particles;
    self.particle_budget = raw.particle_budget;
  }
}

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

/// Rows of the stats overlay: entities, sprites, batches, particles and
/// the separator
const STATS_ROWS: f32 = 5.0;

/// Distance between overlay rows: a line of text plus the label padding
/// above and below, so the panels touch instead of overlapping
pub(crate) fn row_pitch(ui: &UiContext) -> f32 {
  ui.line_height(&overlay_style()) + 8.0
}

/// Height of the frame timing overlay (5 rows + margin)
fn frame_timing_height(ui: &UiContext) -> f32 {
  row_pitch(ui) * 5.0 + OVERLAY_MARGIN
}

/// The y of the top row of the engine overlays shown (stats, FPS, frame
/// timing), or `screen_height` when none is: the rows above it are free.
/// Follows `render_stats`, `render_fps_counter` and `render_frame_timing`.
pub(crate) fn overlay_stack_top(
  ui: &UiContext,
  flags: (bool, bool, bool),
  screen_height: u32,
) -> f32 {
  let (stats, fps, timing) = flags;
  let height = screen_height as f32;
  let pitch = row_pitch(ui);
  let mut top = height;
  if timing {
    top = top.min(height - frame_timing_height(ui));
  }
  if fps {
    let offset = if timing { frame_timing_height(ui) } else { 0.0 };
    top = top.min(height - pitch - offset);
  }
  if stats {
    top = top.min(height - frame_timing_height(ui) - pitch * STATS_ROWS - OVERLAY_MARGIN);
  }
  top
}

/// A mono label on a translucent panel padded to fit the measured text
pub(crate) fn overlay_label(ui: &mut UiContext, text: &str, x: f32, y: f32, color: Color) {
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
pub(crate) fn render_fps_counter(
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
pub(crate) fn render_frame_timing(
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

/// Render debug stats (entity, sprite, batch and particle counts) in the
/// bottom-left corner.
///
/// Positioned above the frame timing overlay (FPS counter is not shown when stats are visible).
pub(crate) fn render_stats(ui: &mut UiContext, stats: &SmoothedDebugStats, screen_height: u32) {
  let pitch = row_pitch(ui);
  // Position above frame timing only (FPS counter not shown when frame timing is visible)
  let base_y = screen_height as f32 - frame_timing_height(ui) - pitch * STATS_ROWS - OVERLAY_MARGIN;
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
    (
      format!(
        "Particles: {} / {}",
        stats.particles.round() as u32,
        stats.particle_budget
      ),
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
      .commands()
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

  #[cfg(feature = "default-fonts")]
  #[test]
  fn the_stats_show_particles_against_the_budget() {
    use crate::core::ui::immediate::{UiCommand, UiCommands};
    let (mut commands, mut text) = (
      UiCommands::default(),
      crate::core::ui::test_util::text_engine(),
    );
    let mut ui = crate::core::ui::test_util::context(&mut commands, &mut text, (1280.0, 720.0));
    let raw = DebugStats {
      particles: 1234,
      particle_budget: 20_000,
      ..DebugStats::default()
    };
    let mut stats = SmoothedDebugStats::default();
    stats.update(&raw);
    assert_eq!(stats.particle_budget, 20_000, "the budget is not smoothed");
    for _ in 0..400 {
      stats.update(&raw);
    }
    render_stats(&mut ui, &stats, 720);
    render_frame_timing(&mut ui, &SmoothedFrameTimings::default(), 720);
    let top = overlay_stack_top(&ui, (true, false, true), 720);
    drop(ui);
    let texts: Vec<String> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Text { layout, .. } => Some(layout.text().to_string()),
        _ => None,
      })
      .collect();
    assert!(
      texts.iter().any(|t| t == "Particles: 1234 / 20000"),
      "{texts:?}"
    );
    // Five stats rows over the frame timing's five, clear of each other,
    // the top row where the stack's top says
    let panels: Vec<Rect> = commands
      .commands()
      .filter_map(|c| match c {
        UiCommand::Rect { rect, .. } => Some(*rect),
        _ => None,
      })
      .collect();
    assert_eq!(panels.len(), 10);
    for pair in panels.windows(2) {
      assert!(
        pair[0].y + pair[0].height <= pair[1].y + 0.01,
        "rows overlap: {pair:?}"
      );
    }
    // A panel is padded 4 px above its text
    assert!((panels[0].y + 4.0 - top).abs() < 0.01, "{top} {panels:?}");
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
      ..DebugStats::default()
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
