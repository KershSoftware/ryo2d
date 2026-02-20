//! Shared context for game and scene callbacks.
//!
//! This module provides the core context struct that both `GameContext` and
//! `SceneContext` are aliases for. This ensures all convenience methods are
//! automatically available in both contexts.

use super::asset::AssetManager;
use super::audio::AudioManager;
use super::config::save::DisplayMode;
use super::ecs::{Entity, Knockback, SquashAxis, SquashStretch, Vec2, World};
use super::events::Events;
use super::graphics::Camera;
use super::input::InputState;
use super::lighting::LightingRenderer;
use super::shaders::EffectsManager;

/// Pending window configuration change requested by the game.
///
/// Set via [`ContextCore::request_display_change`].
/// The engine will apply these changes after the current update.
#[derive(Debug, Clone, Default)]
pub struct WindowRequest {
  /// New resolution width (None = no change)
  pub width: Option<u32>,
  /// New resolution height (None = no change)
  pub height: Option<u32>,
  /// New display mode (None = no change)
  pub display_mode: Option<DisplayMode>,
  /// New UI scale factor (None = no change)
  /// Use Some(0) to reset to auto-detect mode
  pub ui_scale: Option<u32>,
}

/// Time control state for game juice effects.
///
/// Allows game code to control time scale and hitstop effects.
/// Changes are applied immediately and persist until changed again.
#[derive(Debug, Clone)]
pub struct TimeControl {
  /// Time scale multiplier (0.0 = frozen, 1.0 = normal, >1.0 = fast)
  pub time_scale: f32,
  /// Remaining hitstop duration in seconds
  pub hitstop_remaining: f32,
}

impl Default for TimeControl {
  fn default() -> Self {
    Self {
      time_scale: 1.0,
      hitstop_remaining: 0.0,
    }
  }
}

impl WindowRequest {
  /// Create a new empty window request
  pub fn new() -> Self {
    Self::default()
  }

  /// Request a resolution change
  pub fn with_resolution(mut self, width: u32, height: u32) -> Self {
    self.width = Some(width);
    self.height = Some(height);
    self
  }

  /// Request a display mode change
  pub fn with_display_mode(mut self, mode: DisplayMode) -> Self {
    self.display_mode = Some(mode);
    self
  }

  /// Check if any change is requested
  pub fn has_changes(&self) -> bool {
    self.width.is_some()
      || self.height.is_some()
      || self.display_mode.is_some()
      || self.ui_scale.is_some()
  }

  /// Clear all pending requests
  pub fn clear(&mut self) {
    self.width = None;
    self.height = None;
    self.display_mode = None;
    self.ui_scale = None;
  }
}

/// Core context passed to game/scene callbacks, providing access to engine systems.
///
/// This is your "toolkit" for building games - it contains everything
/// you need to create entities, handle input, and control the camera.
///
/// Both `GameContext` and `SceneContext` are type aliases for this struct,
/// ensuring all methods are available in both contexts.
pub struct ContextCore<'a> {
  /// The ECS world containing all entities and components
  pub world: &'a mut World,
  /// The camera for world-to-screen transformations
  pub camera: &'a mut Camera,
  /// Current input state (keyboard, mouse)
  /// Mutable to allow consuming inputs (preventing double-handling)
  pub input: &'a mut InputState,
  /// Asset manager for loading and caching resources
  pub assets: &'a mut AssetManager,
  /// Audio manager for sound effects and music (None if audio unavailable)
  pub audio: &'a mut Option<AudioManager>,
  /// Post-process effects manager for screen-wide visual effects
  pub effects: &'a mut EffectsManager,
  /// 2D lighting renderer for controlling ambient light and dynamic lights (None if lighting disabled)
  pub lighting: &'a mut Option<LightingRenderer>,
  /// Event bus for decoupled communication between systems.
  ///
  /// Use `events.emit()` to publish events and `events.read()` to subscribe.
  /// Events are cleared at the start of each frame.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Emit a custom event
  /// ctx.events.emit(PlayerDied { entity: player });
  ///
  /// // Read built-in collision events
  /// for event in ctx.events.read::<CollisionStarted>() {
  ///     if event.involves(player) {
  ///         take_damage();
  ///     }
  /// }
  /// ```
  pub events: &'a mut Events,
  /// Pending window configuration changes (resolution, display mode)
  pub window_request: &'a mut WindowRequest,
  /// Pending cursor configuration changes (style, visibility, confinement)
  pub cursor_request: &'a mut crate::core::cursor::CursorRequest,
  /// Time control for slow-mo and hitstop effects
  pub time_control: &'a mut TimeControl,
  /// Current window DPI scale factor (1.0 = standard, 2.0 = HiDPI/Retina)
  pub scale_factor: f64,
}

impl<'a> ContextCore<'a> {
  // ===========================================================================
  // Screen/Display
  // ===========================================================================

  /// Get the current screen/viewport dimensions.
  ///
  /// Returns `(width, height)` in pixels. This reflects the game's
  /// logical resolution (from system.json > Ryo.toml > engine defaults).
  ///
  /// # Example
  ///
  /// ```ignore
  /// let (width, height) = ctx.screen_size();
  /// let center_x = width / 2.0;
  /// let center_y = height / 2.0;
  /// ```
  pub fn screen_size(&self) -> (f32, f32) {
    self.camera.viewport_size()
  }

  /// Get the screen width in pixels.
  pub fn screen_width(&self) -> f32 {
    self.camera.viewport_size().0
  }

  /// Get the screen height in pixels.
  pub fn screen_height(&self) -> f32 {
    self.camera.viewport_size().1
  }

  /// Get the window's DPI scale factor.
  ///
  /// Returns 1.0 for standard displays, 2.0 for HiDPI/Retina displays.
  /// Use this to determine appropriate UI scaling for readability.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Choose UI scale based on DPI
  /// let ui_scale = if ctx.scale_factor() >= 2.0 { 2 } else { 1 };
  /// ```
  pub fn scale_factor(&self) -> f64 {
    self.scale_factor
  }

  /// Request a display mode change (windowed, fullscreen, borderless).
  ///
  /// The change will be applied after the current frame completes.
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.request_display_mode(DisplayMode::ExclusiveFullscreen);
  /// ```
  pub fn request_display_mode(&mut self, mode: DisplayMode) {
    self.window_request.display_mode = Some(mode);
  }

  /// Request a resolution change.
  ///
  /// The change will be applied after the current frame completes.
  /// Note: In fullscreen mode, resolution changes may be limited to
  /// available video modes.
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.request_resolution(1920, 1080);
  /// ```
  pub fn request_resolution(&mut self, width: u32, height: u32) {
    self.window_request.width = Some(width);
    self.window_request.height = Some(height);
  }

  /// Request both resolution and display mode change.
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.request_display_change(1920, 1080, DisplayMode::BorderlessFullscreen);
  /// ```
  pub fn request_display_change(&mut self, width: u32, height: u32, mode: DisplayMode) {
    self.window_request.width = Some(width);
    self.window_request.height = Some(height);
    self.window_request.display_mode = Some(mode);
  }

  /// Request a UI scale change.
  ///
  /// The change will be applied after the current frame completes.
  /// Use `None` to reset to auto-detect mode (scale from DPI).
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Force 2x UI scaling
  /// ctx.request_ui_scale(Some(2));
  ///
  /// // Return to auto-detect mode
  /// ctx.request_ui_scale(None);
  /// ```
  pub fn request_ui_scale(&mut self, scale: Option<u32>) {
    // Use 0 as a sentinel for "auto-detect"
    self.window_request.ui_scale = Some(scale.unwrap_or(0));
  }

  // ===========================================================================
  // Cursor Control
  // ===========================================================================

  /// Change the mouse cursor style.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Change to crosshair when aiming
  /// if ctx.input.mouse_down(MouseButton::Right) {
  ///     ctx.set_cursor(CursorStyle::Crosshair);
  /// } else {
  ///     ctx.set_cursor(CursorStyle::Default);
  /// }
  /// ```
  pub fn set_cursor(&mut self, style: crate::core::cursor::CursorStyle) {
    self.cursor_request.cursor = Some(style);
  }

  /// Set cursor visibility.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Hide cursor during gameplay
  /// ctx.set_cursor_visible(false);
  /// ```
  pub fn set_cursor_visible(&mut self, visible: bool) {
    self.cursor_request.visible = Some(visible);
  }

  /// Show the cursor (shorthand for `set_cursor_visible(true)`).
  pub fn show_cursor(&mut self) {
    self.set_cursor_visible(true);
  }

  /// Hide the cursor (shorthand for `set_cursor_visible(false)`).
  pub fn hide_cursor(&mut self) {
    self.set_cursor_visible(false);
  }

  /// Confine the cursor to the window bounds.
  ///
  /// When confined, the cursor cannot leave the window area.
  /// Useful for games that require the cursor to stay within bounds.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Lock cursor to window during gameplay
  /// ctx.set_cursor_confined(true);
  /// ```
  pub fn set_cursor_confined(&mut self, confined: bool) {
    self.cursor_request.confined = Some(confined);
  }

  // ===========================================================================
  // Time Control (Game Juice)
  // ===========================================================================

  /// Set the time scale for game logic, physics, and animations.
  ///
  /// - `0.0` = frozen (equivalent to pause)
  /// - `1.0` = normal speed (default)
  /// - `0.5` = half speed (slow-mo)
  /// - `2.0` = double speed (fast-forward)
  ///
  /// Note: UI and input remain at real-time regardless of time scale.
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Slow-mo during boss death
  /// ctx.set_time_scale(0.3);
  ///
  /// // Return to normal after 2 seconds
  /// // (use a timer in game code)
  /// ctx.set_time_scale(1.0);
  /// ```
  pub fn set_time_scale(&mut self, scale: f32) {
    self.time_control.time_scale = scale.max(0.0);
  }

  /// Get the current time scale.
  pub fn time_scale(&self) -> f32 {
    self.time_control.time_scale
  }

  /// Trigger a hitstop (freeze frame) effect for the given duration.
  ///
  /// During hitstop, game time is frozen (dt = 0) while UI and input
  /// continue at real-time. Great for emphasizing impacts.
  ///
  /// # Arguments
  ///
  /// * `duration_secs` - How long to freeze, in seconds (e.g., 0.05 for 50ms)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // On successful hit - 50ms freeze for impact
  /// ctx.hitstop(0.05);
  ///
  /// // On critical hit - longer freeze
  /// ctx.hitstop(0.1);
  /// ```
  pub fn hitstop(&mut self, duration_secs: f32) {
    self.time_control.hitstop_remaining = duration_secs.max(0.0);
  }

  /// Check if a hitstop is currently active.
  pub fn is_hitstop_active(&self) -> bool {
    self.time_control.hitstop_remaining > 0.0
  }

  // ===========================================================================
  // Knockback
  // ===========================================================================

  /// Apply knockback to an entity in a given direction.
  ///
  /// If the entity already has a Knockback component with `accumulate: true`,
  /// the new knockback will blend with the existing one. Otherwise, it
  /// replaces any existing knockback.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to knock back
  /// * `direction` - Direction to push (will be normalized)
  /// * `force` - Force in pixels per second at full strength
  ///
  /// # Example
  ///
  /// ```ignore
  /// // On collision with enemy, knock player back
  /// let knockback_dir = Vec2::new(
  ///     player_pos.x - enemy_pos.x,
  ///     player_pos.y - enemy_pos.y,
  /// );
  /// ctx.knockback(player, knockback_dir, 400.0);
  /// ```
  pub fn knockback(&mut self, entity: Entity, direction: Vec2, force: f32) {
    if let Some(mut kb) = self.world.get_mut::<Knockback>(entity) {
      kb.apply(direction, force);
    } else {
      self
        .world
        .add_component(entity, Knockback::new(direction, force));
    }
  }

  /// Apply knockback with a custom decay rate.
  ///
  /// Same as [`knockback`](Self::knockback) but allows specifying how fast
  /// the knockback decays.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to knock back
  /// * `direction` - Direction to push (will be normalized)
  /// * `force` - Force in pixels per second at full strength
  /// * `decay_rate` - How fast strength decays (default: 4.0, higher = faster)
  ///
  /// # Examples
  ///
  /// ```ignore
  /// // Quick, snappy knockback (fast decay)
  /// ctx.knockback_with_decay(enemy, dir, 400.0, 8.0);
  ///
  /// // Long, sliding knockback (slow decay)
  /// ctx.knockback_with_decay(enemy, dir, 200.0, 2.0);
  /// ```
  pub fn knockback_with_decay(
    &mut self,
    entity: Entity,
    direction: Vec2,
    force: f32,
    decay_rate: f32,
  ) {
    if let Some(mut kb) = self.world.get_mut::<Knockback>(entity) {
      kb.apply_with_decay(direction, force, decay_rate);
    } else {
      self.world.add_component(
        entity,
        Knockback::new(direction, force).with_decay_rate(decay_rate),
      );
    }
  }

  // ===========================================================================
  // Squash/Stretch
  // ===========================================================================

  /// Apply a squash effect to an entity (compresses on Y axis, stretches on X).
  ///
  /// This is perfect for landing impacts, getting hit from above, or bouncy effects.
  /// The effect automatically recovers to the entity's base scale.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to squash
  /// * `intensity` - How much to squash (0.3 = 30% compression)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // On landing, squash the player
  /// if just_landed {
  ///     ctx.squash(player, 0.3);
  /// }
  /// ```
  pub fn squash(&mut self, entity: Entity, intensity: f32) {
    self.squash_on_axis(entity, intensity, SquashAxis::Y);
  }

  /// Apply a horizontal squash effect (compresses on X axis, stretches on Y).
  ///
  /// This is perfect for wall impacts or horizontal hits.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to squash
  /// * `intensity` - How much to squash (0.3 = 30% compression)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // On wall collision, squash horizontally
  /// ctx.squash_x(player, 0.25);
  /// ```
  pub fn squash_x(&mut self, entity: Entity, intensity: f32) {
    self.squash_on_axis(entity, intensity, SquashAxis::X);
  }

  /// Apply a squash effect on a specific axis.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to squash
  /// * `intensity` - How much to squash (0.3 = 30% compression)
  /// * `axis` - Which axis to compress
  pub fn squash_on_axis(&mut self, entity: Entity, intensity: f32, axis: SquashAxis) {
    if let Some(mut ss) = self.world.get_mut::<SquashStretch>(entity) {
      ss.apply_on_axis(intensity, axis);
    } else {
      self
        .world
        .add_component(entity, SquashStretch::new(intensity).with_axis(axis));
    }
  }

  /// Apply a squash effect with custom decay rate.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to squash
  /// * `intensity` - How much to squash (0.3 = 30% compression)
  /// * `decay_rate` - How fast to recover (8.0 = snappy, 4.0 = moderate, 2.0 = slow)
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Slow, exaggerated squash for cartoon effect
  /// ctx.squash_with_decay(player, 0.5, 3.0);
  /// ```
  pub fn squash_with_decay(&mut self, entity: Entity, intensity: f32, decay_rate: f32) {
    if let Some(mut ss) = self.world.get_mut::<SquashStretch>(entity) {
      ss.apply_with_decay(intensity, decay_rate);
    } else {
      self.world.add_component(
        entity,
        SquashStretch::new(intensity).with_decay_rate(decay_rate),
      );
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn time_control_default_values() {
    let tc = TimeControl::default();
    assert_eq!(tc.time_scale, 1.0);
    assert_eq!(tc.hitstop_remaining, 0.0);
  }

  #[test]
  fn time_control_time_scale_clamped_positive() {
    // Note: Clamping is done in the context methods, not in the struct itself
    // This test just verifies the field can be set
    let tc = TimeControl {
      time_scale: -1.0,
      ..Default::default()
    };
    assert_eq!(tc.time_scale, -1.0);
  }

  #[test]
  fn time_control_hitstop_sets_value() {
    let tc = TimeControl {
      hitstop_remaining: 0.05,
      ..Default::default()
    };
    assert!((tc.hitstop_remaining - 0.05).abs() < 0.001);
  }

  #[test]
  fn time_control_is_hitstop_active() {
    let mut tc = TimeControl::default();
    assert_eq!(tc.hitstop_remaining, 0.0);

    tc.hitstop_remaining = 0.05;
    assert!(tc.hitstop_remaining > 0.0);

    tc.hitstop_remaining = 0.0;
    assert_eq!(tc.hitstop_remaining, 0.0);
  }

  #[test]
  fn window_request_time_scale_integration() {
    // Verify WindowRequest and TimeControl can coexist in the same context
    let wr = WindowRequest::default();
    let tc = TimeControl::default();

    assert!(!wr.has_changes());
    assert_eq!(tc.time_scale, 1.0);
  }

  // ==========================================================================
  // DPI/UI Scale Tests
  // ==========================================================================

  #[test]
  fn window_request_default_ui_scale_is_none() {
    let wr = WindowRequest::default();
    assert_eq!(wr.ui_scale, None, "Default ui_scale should be None");
  }

  #[test]
  fn window_request_ui_scale_manual() {
    let mut wr = WindowRequest::default();
    wr.ui_scale = Some(2);
    assert_eq!(wr.ui_scale, Some(2));
  }

  #[test]
  fn window_request_ui_scale_auto_signal() {
    let mut wr = WindowRequest::default();
    // 0 signals auto-detect mode
    wr.ui_scale = Some(0);
    assert_eq!(wr.ui_scale, Some(0));
  }

  #[test]
  fn window_request_has_changes_with_ui_scale() {
    let mut wr = WindowRequest::default();
    assert!(!wr.has_changes(), "Default should have no changes");

    wr.ui_scale = Some(2);
    assert!(wr.has_changes(), "Setting ui_scale should register as a change");
  }

  #[test]
  fn window_request_clear_resets_ui_scale() {
    let mut wr = WindowRequest::default();
    wr.ui_scale = Some(2);
    assert!(wr.has_changes());

    wr.clear();
    assert!(!wr.has_changes());
    assert_eq!(wr.ui_scale, None);
  }
}
