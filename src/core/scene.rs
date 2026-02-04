//! Scene management system for organizing game states
//!
//! Scenes represent distinct states of your game (menu, gameplay, pause, etc.).
//! The `SceneManager` maintains a stack of scenes, allowing for:
//! - Push/pop navigation (menu → gameplay → pause → resume)
//! - Scene transitions with optional fade effects
//! - Clean separation of game state logic
//!
//! # Example
//!
//! ```rust,ignore
//! struct MenuScene;
//!
//! impl Scene for MenuScene {
//!     fn on_enter(&mut self, ctx: &mut SceneContext) {
//!         println!("Menu activated");
//!     }
//!
//!     fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
//!         if ctx.input.key_pressed(KeyCode::Enter) {
//!             return SceneAction::Push(Box::new(GameplayScene::new()));
//!         }
//!         SceneAction::Continue
//!     }
//! }
//! ```

use std::time::Duration;

use crate::profile_scope;

use super::asset::AssetManager;
use super::audio::AudioManager;
use super::config::save::DisplayMode;
use super::console::{CommandRegistry, CommandResult};
use super::ecs::{Entity, Knockback, SquashAxis, SquashStretch, Vec2, World};
use super::events::Events;
use super::game::{TimeControl, WindowRequest};
use super::graphics::Camera;
use super::input::InputState;
use super::lighting::LightingRenderer;
use super::shaders::EffectsManager;
use super::ui::UiContext;

/// Context passed to scene callbacks, providing access to engine systems.
///
/// Similar to `GameContext` but designed for scene-based architecture.
pub struct SceneContext<'a> {
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
  /// ctx.events.emit(EnemyDied { entity: enemy, score: 100 });
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
  pub(crate) cursor_request: &'a mut crate::core::cursor::CursorRequest,
  /// Time control for slow-mo and hitstop effects
  pub time_control: &'a mut TimeControl,
  /// Whether detailed profiling is enabled (for timing warnings)
  pub profiling_enabled: bool,
  /// Current window DPI scale factor (1.0 = standard, 2.0 = HiDPI/Retina)
  pub scale_factor: f64,
  /// Console command registry for registering custom commands
  pub(crate) command_registry: &'a mut CommandRegistry,
  /// Unique scope ID for this context (used for command handler cleanup)
  pub(crate) scope_id: u64,
}

impl<'a> SceneContext<'a> {
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
  /// ctx.request_display_mode(DisplayMode::Fullscreen);
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
  ///
  /// # Arguments
  ///
  /// * `scale` - The new UI scale factor, or `None` to auto-detect from DPI
  ///
  /// # Example
  ///
  /// ```ignore
  /// // Force 2x UI scale for HiDPI
  /// ctx.request_ui_scale(Some(2));
  ///
  /// // Reset to auto-detect from DPI
  /// ctx.request_ui_scale(None);
  /// ```
  pub fn request_ui_scale(&mut self, scale: Option<u32>) {
    // Use 0 to signal auto-detect mode
    self.window_request.ui_scale = Some(scale.unwrap_or(0));
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
  // Cursor Control
  // ===========================================================================

  /// Set the mouse cursor style.
  ///
  /// Changes are applied after the current frame completes.
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.set_cursor(CursorStyle::Pointer);  // For clickable elements
  /// ctx.set_cursor(CursorStyle::Crosshair);  // For aiming
  /// ```
  pub fn set_cursor(&mut self, style: crate::core::cursor::CursorStyle) {
    self.cursor_request.cursor = Some(style);
  }

  /// Set whether the mouse cursor is visible.
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.set_cursor_visible(false);  // Hide for first-person games
  /// ```
  pub fn set_cursor_visible(&mut self, visible: bool) {
    self.cursor_request.visible = Some(visible);
  }

  /// Show the mouse cursor.
  ///
  /// Shorthand for `set_cursor_visible(true)`.
  pub fn show_cursor(&mut self) {
    self.set_cursor_visible(true);
  }

  /// Hide the mouse cursor.
  ///
  /// Shorthand for `set_cursor_visible(false)`.
  pub fn hide_cursor(&mut self) {
    self.set_cursor_visible(false);
  }

  /// Set whether the mouse cursor is confined to the window.
  ///
  /// When confined, the cursor cannot leave the window bounds.
  /// Useful for first-person games or when using relative mouse input.
  ///
  /// # Example
  ///
  /// ```ignore
  /// ctx.set_cursor_confined(true);  // Lock cursor to window
  /// ```
  pub fn set_cursor_confined(&mut self, confined: bool) {
    self.cursor_request.confined = Some(confined);
  }

  /// Set a custom cursor from a sprite.
  ///
  /// The hotspot is the point within the cursor image that represents
  /// the actual click position (in pixels from top-left).
  ///
  /// # Example
  ///
  /// ```ignore
  /// let crosshair = ctx.assets.load_sprite("crosshair.png")?;
  /// ctx.set_custom_cursor(&crosshair, 16, 16);  // Hotspot at center of 32x32 cursor
  /// ```
  pub fn set_custom_cursor(
    &mut self,
    sprite: &crate::core::asset::SpriteHandle,
    hotspot_x: u16,
    hotspot_y: u16,
  ) {
    sprite.with_sprite(|s| {
      self.cursor_request.custom_cursor = Some(crate::core::cursor::CustomCursorData {
        rgba: s.data().to_vec(),
        width: s.width() as u16,
        height: s.height() as u16,
        hotspot_x,
        hotspot_y,
        id: sprite.id(),
      });
    });
  }

  /// Set a custom cursor with a hotspot preset.
  ///
  /// # Example
  ///
  /// ```ignore
  /// let crosshair = ctx.assets.load_sprite("crosshair.png")?;
  /// ctx.set_custom_cursor_centered(&crosshair);  // Hotspot at center
  /// ```
  pub fn set_custom_cursor_centered(&mut self, sprite: &crate::core::asset::SpriteHandle) {
    let hotspot_x = (sprite.width() / 2) as u16;
    let hotspot_y = (sprite.height() / 2) as u16;
    self.set_custom_cursor(sprite, hotspot_x, hotspot_y);
  }

  // ===========================================================================
  // Knockback
  // ===========================================================================

  /// Apply knockback to an entity with custom decay rate.
  ///
  /// Knockback pushes an entity in a direction with an initial force that
  /// decays over time. This creates satisfying impact feedback.
  ///
  /// # Arguments
  ///
  /// * `entity` - The entity to knock back
  /// * `direction` - Direction to push (will be normalized)
  /// * `force` - Initial force/speed in pixels per second
  /// * `decay_rate` - How fast strength decays (default: 4.0, higher = faster)
  pub fn knockback_with_decay(
    &mut self,
    entity: Entity,
    direction: Vec2,
    force: f32,
    decay_rate: f32,
  ) {
    if let Some(mut kb) = self.world.get_component_mut::<Knockback>(entity) {
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
  pub fn squash(&mut self, entity: Entity, intensity: f32) {
    self.squash_on_axis(entity, intensity, SquashAxis::Y);
  }

  /// Apply a horizontal squash effect (compresses on X axis, stretches on Y).
  ///
  /// This is perfect for wall impacts or horizontal hits.
  pub fn squash_x(&mut self, entity: Entity, intensity: f32) {
    self.squash_on_axis(entity, intensity, SquashAxis::X);
  }

  /// Apply a squash effect on a specific axis.
  pub fn squash_on_axis(&mut self, entity: Entity, intensity: f32, axis: SquashAxis) {
    if let Some(mut ss) = self.world.get_component_mut::<SquashStretch>(entity) {
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
  pub fn squash_with_decay(&mut self, entity: Entity, intensity: f32, decay_rate: f32) {
    if let Some(mut ss) = self.world.get_component_mut::<SquashStretch>(entity) {
      ss.apply_with_decay(intensity, decay_rate);
    } else {
      self.world.add_component(
        entity,
        SquashStretch::new(intensity).with_decay_rate(decay_rate),
      );
    }
  }

  // ===========================================================================
  // Developer Console
  // ===========================================================================

  /// Declare a game console command.
  ///
  /// Declared commands appear in help immediately but are "noop" until a handler
  /// is registered. This can be called from any scene but is typically done
  /// in the first scene or a setup scene.
  ///
  /// Note: Declarations are global and idempotent - calling this multiple times
  /// with the same name just updates the description.
  ///
  /// # Arguments
  ///
  /// * `name` - The command name (should start with "game:" prefix)
  /// * `description` - Brief description shown in command listings
  /// * `verbose_description` - Optional detailed help shown for `help <command>`
  pub fn declare_console_command(
    &mut self,
    name: impl Into<String>,
    description: impl Into<String>,
    verbose_description: Option<impl Into<String>>,
  ) {
    self.command_registry.declare(name, description, verbose_description);
  }

  /// Register a handler for a declared game command.
  ///
  /// The handler is scoped to this scene - it will be automatically cleaned up
  /// when the scene exits. Commands must be declared first using
  /// `declare_console_command` before registering handlers.
  ///
  /// # Arguments
  ///
  /// * `name` - The command name (must have been declared first)
  /// * `handler` - The function to execute when the command is invoked
  ///
  /// # Returns
  ///
  /// `true` if the command was declared and handler was attached, `false` otherwise.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn on_enter(&mut self, ctx: &mut SceneContext) {
  ///     // First, declare the command (typically done once in first scene)
  ///     ctx.declare_console_command(
  ///         "game:reset",
  ///         "Reset the game",
  ///         Some("Resets all game state. Only works during gameplay."),
  ///     );
  ///
  ///     // Register handler - will be auto-cleaned when this scene exits
  ///     ctx.register_console_command("game:reset", |_args, cmd_ctx| {
  ///         cmd_ctx.console.print("Resetting game...");
  ///         CommandResult::Ok
  ///     });
  /// }
  /// ```
  pub fn register_console_command<F>(&mut self, name: &str, handler: F) -> bool
  where
    F: Fn(&[&str], &mut super::console::CommandContext) -> CommandResult + Send + Sync + 'static,
  {
    self.command_registry.register_handler(name, self.scope_id, handler)
  }
}

// ============================================================================
// Transitions
// ============================================================================

/// Visual transition effect when switching between scenes.
///
/// Transitions provide smooth visual effects during scene changes,
/// improving the user experience.
///
/// # Example
///
/// ```rust,ignore
/// // Fade to black over 500ms, then fade in the new scene
/// SceneAction::SwitchWith(
///     Box::new(GameplayScene::new()),
///     Transition::fade(Duration::from_millis(500))
/// )
/// ```
#[derive(Debug, Clone, Default)]
pub enum Transition {
  /// Instant transition (no effect)
  #[default]
  None,
  /// Fade through a solid color (default: black)
  Fade {
    /// Duration of the fade out (old scene)
    duration: Duration,
    /// Color to fade through (RGBA)
    color: [u8; 4],
  },
  /// Slide the new scene in from a direction
  Slide {
    /// Duration of the slide
    duration: Duration,
    /// Direction to slide from
    direction: SlideDirection,
  },
}

impl Transition {
  /// Create a fade transition through black.
  pub fn fade(duration: Duration) -> Self {
    Transition::Fade {
      duration,
      color: [0, 0, 0, 255], // Black
    }
  }

  /// Create a fade transition through a custom color.
  pub fn fade_color(duration: Duration, r: u8, g: u8, b: u8) -> Self {
    Transition::Fade {
      duration,
      color: [r, g, b, 255],
    }
  }

  /// Create a fade transition through white.
  pub fn fade_white(duration: Duration) -> Self {
    Transition::Fade {
      duration,
      color: [255, 255, 255, 255],
    }
  }

  /// Create a slide transition.
  pub fn slide(duration: Duration, direction: SlideDirection) -> Self {
    Transition::Slide {
      duration,
      direction,
    }
  }

  /// Get the total duration of this transition.
  pub fn duration(&self) -> Duration {
    match self {
      Transition::None => Duration::ZERO,
      Transition::Fade { duration, .. } => *duration * 2, // fade out + fade in
      Transition::Slide { duration, .. } => *duration,
    }
  }
}

/// Direction for slide transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideDirection {
  /// Slide in from the left
  Left,
  /// Slide in from the right
  Right,
  /// Slide in from the top
  Up,
  /// Slide in from the bottom
  Down,
}

/// Actions a scene can return to control scene flow.
///
/// Return these from `update()` to tell the SceneManager what to do next.
///
/// # Transitions
///
/// Actions with `With` suffix (e.g., `SwitchWith`, `PushWith`) accept a
/// `Transition` for smooth visual effects. Actions without transitions
/// change immediately.
#[derive(Default)]
pub enum SceneAction {
  /// Continue running the current scene (default)
  #[default]
  Continue,
  /// Push a new scene onto the stack (current scene pauses)
  Push(Box<dyn Scene>),
  /// Push with a transition effect
  PushWith(Box<dyn Scene>, Transition),
  /// Pop the current scene off the stack (returns to previous)
  Pop,
  /// Pop with a transition effect
  PopWith(Transition),
  /// Replace the current scene with a new one
  Switch(Box<dyn Scene>),
  /// Switch with a transition effect
  SwitchWith(Box<dyn Scene>, Transition),
  /// Clear all scenes and push a new one
  Reset(Box<dyn Scene>),
  /// Reset with a transition effect
  ResetWith(Box<dyn Scene>, Transition),
  /// Exit the game
  Quit,
}

/// Trait for implementing game scenes.
///
/// Scenes are distinct states of your game, each with their own logic.
/// The SceneManager calls these methods at appropriate times.
///
/// # Lifecycle
///
/// ```text
/// ┌─────────────┐
/// │  on_enter   │  ← Called when scene becomes active
/// └──────┬──────┘
///        ▼
/// ┌─────────────┐
/// │   update    │  ← Called every fixed timestep
/// │   render    │  ← Called every frame (optional)
/// └──────┬──────┘
///        │ (push another scene)
///        ▼
/// ┌─────────────┐
/// │  on_pause   │  ← Called when another scene is pushed on top
/// └──────┬──────┘
///        │ (top scene pops)
///        ▼
/// ┌─────────────┐
/// │  on_resume  │  ← Called when this scene becomes active again
/// └──────┬──────┘
///        │ (pop or switch)
///        ▼
/// ┌─────────────┐
/// │  on_exit    │  ← Called when scene is removed from stack
/// └─────────────┘
/// ```
pub trait Scene {
  /// Called when this scene becomes the active scene.
  ///
  /// Use this to:
  /// - Initialize scene-specific entities
  /// - Set up the camera
  /// - Load assets
  /// - Play background music
  fn on_enter(&mut self, _ctx: &mut SceneContext) {}

  /// Called when this scene is removed from the stack.
  ///
  /// Use this to:
  /// - Clean up entities
  /// - Stop music
  /// - Save state if needed
  fn on_exit(&mut self, _ctx: &mut SceneContext) {}

  /// Called when another scene is pushed on top of this one.
  ///
  /// This scene remains in the stack but is no longer updated.
  /// Use this to pause music, timers, etc.
  fn on_pause(&mut self, _ctx: &mut SceneContext) {}

  /// Called when this scene becomes active again after being paused.
  ///
  /// The scene above was popped, so this scene resumes.
  /// Use this to resume music, timers, etc.
  fn on_resume(&mut self, _ctx: &mut SceneContext) {}

  /// Called every fixed timestep while this scene is active.
  ///
  /// This is where your main scene logic goes. Return a `SceneAction`
  /// to control scene flow (push, pop, switch, etc.).
  ///
  /// # Arguments
  ///
  /// * `ctx` - Access to engine systems
  /// * `dt` - Fixed delta time in seconds
  ///
  /// # Returns
  ///
  /// A `SceneAction` indicating what the SceneManager should do next.
  fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction;

  /// Called every frame for custom rendering (optional).
  ///
  /// Most scenes don't need to override this - the engine's built-in
  /// render system handles Renderable components automatically.
  ///
  /// Override this for:
  /// - Custom UI drawing
  /// - Debug visualizations
  /// - Post-processing effects
  ///
  /// # Arguments
  ///
  /// * `ctx` - Access to engine systems
  /// * `alpha` - Interpolation alpha for smooth rendering (0.0 to 1.0)
  fn render(&mut self, _ctx: &mut SceneContext, _alpha: f32) {}

  /// Called after frame clear but before entity rendering.
  ///
  /// Use this for parallax backgrounds and other background layers that
  /// should appear behind all game entities.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
  ///     self.background.render(frame, width, height, camera);
  /// }
  /// ```
  fn render_background(&mut self, _frame: &mut [u8], _width: u32, _height: u32, _camera: &Camera) {}

  /// Called after sprite rendering to draw UI elements.
  ///
  /// Use this for HUD elements, debug overlays, and in-game UI.
  /// UI is drawn in screen space, on top of all world sprites.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn render_ui(&mut self, ui: &mut UiContext) {
  ///     ui.draw_text("Press ENTER to start", 300.0, 400.0, Color::WHITE);
  ///     if ui.button("Start", Rect::new(350.0, 450.0, 100.0, 30.0)) {
  ///         self.start_game = true;
  ///     }
  /// }
  /// ```
  fn render_ui(&mut self, _ui: &mut UiContext) {}

  /// Called for GPU-accelerated rendering (optional).
  ///
  /// Use this for:
  /// - GPU parallax backgrounds (much faster than CPU)
  /// - Particle systems with many sprites
  /// - Any high-performance instanced rendering
  ///
  /// Sprites added here are batched and rendered with GPU instancing,
  /// which is significantly faster than CPU rendering for large quantities.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn render_gpu(&mut self, ctx: &mut GpuRenderContext) {
  ///     // Upload textures (cached, safe to call every frame)
  ///     ctx.upload_sprite(&self.particle_sprite);
  ///
  ///     // Render GPU parallax background
  ///     self.background.render(ctx);
  /// }
  /// ```
  fn render_gpu(&mut self, _ctx: &mut super::game::GpuRenderContext) {}

  /// Whether scenes below this one should also be rendered.
  ///
  /// Return `true` for transparent/overlay scenes (pause menu, HUD).
  /// Return `false` (default) for opaque scenes that fully cover the screen.
  fn is_transparent(&self) -> bool {
    false
  }

  /// Whether scenes below this one should also be updated.
  ///
  /// Return `true` to allow background scenes to continue updating.
  /// Return `false` (default) to pause scenes below.
  fn updates_below(&self) -> bool {
    false
  }

  /// Human-readable name for debugging and logging.
  ///
  /// Override this to provide a custom name, or leave the default
  /// which uses the type name.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn name(&self) -> &str {
  ///     "MainMenu"
  /// }
  /// ```
  fn name(&self) -> &str {
    std::any::type_name::<Self>()
      .rsplit("::")
      .next()
      .unwrap_or("Unknown")
  }

  /// Whether physics should be paused for this scene.
  ///
  /// Override this to pause physics during pause menus, cutscenes, etc.
  /// Default is `false` (physics runs normally).
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// fn is_physics_paused(&self) -> bool {
  ///     self.is_paused
  /// }
  /// ```
  fn is_physics_paused(&self) -> bool {
    false
  }
}

/// Manages a stack of scenes.
///
/// The scene at the top of the stack is the "active" scene that receives
/// update calls. Scenes below may or may not update/render depending on
/// the `is_transparent()` and `updates_below()` settings of scenes above.
///
/// # Transitions
///
/// Scene changes can include visual transitions (fade, slide).
/// During a transition, both scenes may be rendered with effects applied.
///
/// # Example
///
/// ```rust,ignore
/// let mut manager = SceneManager::new();
///
/// // Start with menu
/// manager.push(Box::new(MenuScene::new()));
///
/// // In your game loop:
/// let action = manager.update(&mut ctx, dt);
/// manager.handle_action(action, &mut ctx);
/// manager.render(&mut ctx, alpha);
/// ```
pub struct SceneManager {
  /// Stack of scenes (back = top)
  scenes: Vec<Box<dyn Scene>>,
  /// Active transition state
  transition_state: Option<TransitionState>,
  /// When true, skip the next update cycle to prevent input leak-through.
  /// Set after scene transitions (pop/switch/reset) so the newly-active
  /// scene doesn't process stale input from the previous scene.
  skip_next_update: bool,
}

/// Internal state for an in-progress transition.
#[derive(Debug)]
struct TransitionState {
  /// The transition effect being applied
  transition: Transition,
  /// Elapsed time in the transition
  elapsed: Duration,
  /// The action to perform at the midpoint (for fades) or end
  pending_action: PendingAction,
  /// Current phase of the transition
  phase: TransitionPhase,
}

enum PendingAction {
  Push(Box<dyn Scene>),
  Pop,
  Switch(Box<dyn Scene>),
  Reset(Box<dyn Scene>),
}

impl std::fmt::Debug for PendingAction {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PendingAction::Push(_) => write!(f, "Push(...)"),
      PendingAction::Pop => write!(f, "Pop"),
      PendingAction::Switch(_) => write!(f, "Switch(...)"),
      PendingAction::Reset(_) => write!(f, "Reset(...)"),
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPhase {
  /// Fading/sliding out the old scene
  Out,
  /// Fading/sliding in the new scene
  In,
}

impl SceneManager {
  /// Create a new empty scene manager.
  pub fn new() -> Self {
    SceneManager {
      scenes: Vec::new(),
      transition_state: None,
      skip_next_update: false,
    }
  }

  /// Create a scene manager with an initial scene.
  pub fn with_scene(scene: Box<dyn Scene>) -> Self {
    let mut manager = Self::new();
    manager.scenes.push(scene);
    manager
  }

  /// Check if a transition is currently in progress.
  pub fn is_transitioning(&self) -> bool {
    self.transition_state.is_some()
  }

  /// Get the current transition progress (0.0 to 1.0).
  ///
  /// Returns `None` if no transition is active.
  pub fn transition_progress(&self) -> Option<f32> {
    self.transition_state.as_ref().map(|state| {
      let total = state.transition.duration();
      if total.is_zero() {
        1.0
      } else {
        (state.elapsed.as_secs_f32() / total.as_secs_f32()).min(1.0)
      }
    })
  }

  /// Get the current transition overlay alpha (0.0 to 1.0).
  ///
  /// For fade transitions, this is the opacity of the fade overlay.
  /// Returns 0.0 if no transition or transition doesn't use overlay.
  pub fn transition_overlay_alpha(&self) -> f32 {
    match &self.transition_state {
      Some(state) => match &state.transition {
        Transition::None => 0.0,
        Transition::Fade { duration, .. } => {
          let half_duration = duration.as_secs_f32();
          let elapsed = state.elapsed.as_secs_f32();

          match state.phase {
            TransitionPhase::Out => (elapsed / half_duration).min(1.0),
            TransitionPhase::In => (1.0 - (elapsed / half_duration)).max(0.0),
          }
        }
        Transition::Slide { .. } => 0.0,
      },
      None => 0.0,
    }
  }

  /// Get the transition overlay color (if any).
  pub fn transition_overlay_color(&self) -> Option<[u8; 4]> {
    match &self.transition_state {
      Some(state) => match &state.transition {
        Transition::Fade { color, .. } => Some(*color),
        _ => None,
      },
      None => None,
    }
  }

  /// Push a new scene onto the stack.
  ///
  /// The current top scene will have `on_pause()` called.
  /// The new scene will have `on_enter()` called.
  pub fn push(&mut self, scene: Box<dyn Scene>, ctx: &mut SceneContext) {
    profile_scope!("cpu:scene_push");

    // Pause current top scene
    if let Some(current) = self.scenes.last_mut() {
      current.on_pause(ctx);
    }

    // Add and enter new scene
    self.scenes.push(scene);
    if let Some(new_top) = self.scenes.last_mut() {
      #[cfg(debug_assertions)]
      let start = if ctx.profiling_enabled {
        Some(std::time::Instant::now())
      } else {
        None
      };

      new_top.on_enter(ctx);

      #[cfg(debug_assertions)]
      if let Some(start) = start {
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 5 {
          eprintln!("[Scene] on_enter took {:?}", elapsed);
        }
      }
    }
  }

  /// Pop the top scene off the stack.
  ///
  /// The popped scene will have `on_exit()` called.
  /// The new top scene (if any) will have `on_resume()` called.
  ///
  /// Returns the popped scene, or None if stack was empty.
  pub fn pop(&mut self, ctx: &mut SceneContext) -> Option<Box<dyn Scene>> {
    if let Some(mut scene) = self.scenes.pop() {
      scene.on_exit(ctx);

      // Resume the new top scene
      if let Some(new_top) = self.scenes.last_mut() {
        new_top.on_resume(ctx);
        // Skip the next update so the resumed scene doesn't process
        // stale input (e.g., the Escape that caused the pop)
        self.skip_next_update = true;
      }

      Some(scene)
    } else {
      None
    }
  }

  /// Replace the current scene with a new one.
  ///
  /// The current scene will have `on_exit()` called.
  /// The new scene will have `on_enter()` called.
  /// Scenes below are unaffected.
  pub fn switch(&mut self, scene: Box<dyn Scene>, ctx: &mut SceneContext) {
    profile_scope!("cpu:scene_switch");

    // Exit current top
    if let Some(mut current) = self.scenes.pop() {
      current.on_exit(ctx);
    }

    // Enter new scene
    self.scenes.push(scene);
    if let Some(new_top) = self.scenes.last_mut() {
      #[cfg(debug_assertions)]
      let start = if ctx.profiling_enabled {
        Some(std::time::Instant::now())
      } else {
        None
      };

      new_top.on_enter(ctx);

      #[cfg(debug_assertions)]
      if let Some(start) = start {
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 5 {
          eprintln!("[Scene] switch on_enter took {:?}", elapsed);
        }
      }
    }
  }

  /// Clear all scenes and optionally push a new one.
  ///
  /// All scenes will have `on_exit()` called (top to bottom).
  pub fn clear(&mut self, ctx: &mut SceneContext) {
    while let Some(mut scene) = self.scenes.pop() {
      scene.on_exit(ctx);
    }
  }

  /// Reset with a new scene (clear all, then push).
  pub fn reset(&mut self, scene: Box<dyn Scene>, ctx: &mut SceneContext) {
    self.clear(ctx);
    self.scenes.push(scene);
    if let Some(new_top) = self.scenes.last_mut() {
      new_top.on_enter(ctx);
    }
  }

  /// Update the active scene(s).
  ///
  /// Returns a `SceneAction` from the top scene's update.
  /// Call `handle_action()` after to process it.
  pub fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
    if self.scenes.is_empty() {
      return SceneAction::Quit;
    }

    // If a scene transition just happened, skip this update to prevent
    // the newly-active scene from processing stale input (e.g., the Escape
    // key that caused the previous scene to pop).
    if self.skip_next_update {
      self.skip_next_update = false;
      return SceneAction::Continue;
    }

    // Determine which scenes should update
    // Work from top down, stop when we hit a scene that doesn't update below
    let mut update_from = self.scenes.len() - 1;
    for i in (0..self.scenes.len()).rev() {
      if i == self.scenes.len() - 1 {
        // Top scene always updates
        continue;
      }
      // Check if the scene above allows updates below
      if !self.scenes[i + 1].updates_below() {
        update_from = i + 1;
        break;
      }
    }

    // Update scenes from bottom-up (so top scene's action wins)
    let mut action = SceneAction::Continue;
    for i in update_from..self.scenes.len() {
      // We need to work around the borrow checker here
      // Get raw pointer to scene, update, then handle action
      let scene = &mut self.scenes[i];
      let result = scene.update(ctx, dt);

      // Only the top scene's action matters
      if i == self.scenes.len() - 1 {
        action = result;
      }
    }

    action
  }

  /// Handle a scene action returned from update.
  ///
  /// This processes push/pop/switch/etc. actions.
  pub fn handle_action(&mut self, action: SceneAction, ctx: &mut SceneContext) -> bool {
    match action {
      SceneAction::Continue => true,
      SceneAction::Push(scene) => {
        self.push(scene, ctx);
        true
      }
      SceneAction::Pop => {
        self.pop(ctx);
        !self.scenes.is_empty()
      }
      SceneAction::Switch(scene) => {
        self.switch(scene, ctx);
        true
      }
      SceneAction::Reset(scene) => {
        self.reset(scene, ctx);
        true
      }
      // Transition variants
      SceneAction::PushWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Push(scene));
        true
      }
      SceneAction::PopWith(transition) => {
        self.start_transition(transition, PendingAction::Pop);
        true
      }
      SceneAction::SwitchWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Switch(scene));
        true
      }
      SceneAction::ResetWith(scene, transition) => {
        self.start_transition(transition, PendingAction::Reset(scene));
        true
      }
      SceneAction::Quit => false,
    }
  }

  /// Start a transition effect.
  fn start_transition(&mut self, transition: Transition, action: PendingAction) {
    if matches!(transition, Transition::None) {
      // No transition, just a marker - shouldn't happen but handle it
      return;
    }

    self.transition_state = Some(TransitionState {
      transition,
      elapsed: Duration::ZERO,
      pending_action: action,
      phase: TransitionPhase::Out,
    });
  }

  /// Update transition state, performing the scene change at the right moment.
  ///
  /// Call this each frame with the delta time to advance transitions.
  /// Returns `true` if the game should continue, `false` if it should quit.
  pub fn update_transition(&mut self, dt: f32, ctx: &mut SceneContext) -> bool {
    let dt_duration = Duration::from_secs_f32(dt);

    // First pass: update elapsed time and check what action to take
    let action_to_execute = if let Some(ref mut state) = self.transition_state {
      state.elapsed += dt_duration;

      match &state.transition {
        Transition::None => {
          // Shouldn't be here, but clean up
          self.transition_state = None;
          None
        }
        Transition::Fade { duration, .. } => {
          let duration = *duration;
          match state.phase {
            TransitionPhase::Out => {
              if state.elapsed >= duration {
                // Need to execute action and switch to In phase
                state.elapsed = Duration::ZERO;
                state.phase = TransitionPhase::In;
                Some(std::mem::replace(
                  &mut state.pending_action,
                  PendingAction::Pop,
                ))
              } else {
                None
              }
            }
            TransitionPhase::In => {
              if state.elapsed >= duration {
                self.transition_state = None;
              }
              None
            }
          }
        }
        Transition::Slide { duration, .. } => {
          let duration = *duration;
          if state.phase == TransitionPhase::Out {
            state.phase = TransitionPhase::In;
            Some(std::mem::replace(
              &mut state.pending_action,
              PendingAction::Pop,
            ))
          } else if state.elapsed >= duration {
            self.transition_state = None;
            None
          } else {
            None
          }
        }
      }
    } else {
      None
    };

    // Second pass: execute any pending action
    if let Some(action) = action_to_execute {
      self.execute_pending_action(action, ctx);
    }

    !self.scenes.is_empty()
  }

  /// Execute a pending action during a transition.
  fn execute_pending_action(&mut self, action: PendingAction, ctx: &mut SceneContext) {
    match action {
      PendingAction::Push(scene) => {
        self.push(scene, ctx);
      }
      PendingAction::Pop => {
        self.pop(ctx);
      }
      PendingAction::Switch(scene) => {
        self.switch(scene, ctx);
      }
      PendingAction::Reset(scene) => {
        self.reset(scene, ctx);
      }
    }
  }

  /// Render all visible scenes.
  ///
  /// Renders from bottom to top, respecting `is_transparent()`.
  pub fn render(&mut self, ctx: &mut SceneContext, alpha: f32) {
    if self.scenes.is_empty() {
      return;
    }

    // Find the first scene that's not transparent (render from there)
    let mut render_from = self.scenes.len() - 1;
    for i in (0..self.scenes.len()).rev() {
      render_from = i;
      if i == 0 || !self.scenes[i].is_transparent() {
        break;
      }
    }

    // Render from bottom-up
    for i in render_from..self.scenes.len() {
      self.scenes[i].render(ctx, alpha);
    }
  }

  /// Render UI for all visible scenes.
  ///
  /// Called after sprite rendering. Renders from bottom to top.
  pub fn render_ui(&mut self, ui: &mut UiContext) {
    if self.scenes.is_empty() {
      return;
    }

    // Find the first scene that's not transparent (render from there)
    let mut render_from = self.scenes.len() - 1;
    for i in (0..self.scenes.len()).rev() {
      render_from = i;
      if i == 0 || !self.scenes[i].is_transparent() {
        break;
      }
    }

    // Render UI from bottom-up
    for i in render_from..self.scenes.len() {
      self.scenes[i].render_ui(ui);
    }
  }

  /// Render background for the active scene.
  ///
  /// Called after frame clear but before entity rendering.
  /// Only renders background for the top scene.
  pub fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
    if let Some(scene) = self.scenes.last_mut() {
      scene.render_background(frame, width, height, camera);
    }
  }

  /// Render GPU content for the active scene.
  ///
  /// Called during GPU rendering phase for instanced sprite rendering.
  /// Only renders GPU content for the top scene.
  pub fn render_gpu(&mut self, ctx: &mut super::game::GpuRenderContext) {
    if let Some(scene) = self.scenes.last_mut() {
      scene.render_gpu(ctx);
    }
  }

  /// Check if the scene stack is empty.
  pub fn is_empty(&self) -> bool {
    self.scenes.is_empty()
  }

  /// Get the number of scenes on the stack.
  pub fn len(&self) -> usize {
    self.scenes.len()
  }

  /// Get a reference to the top scene (if any).
  #[allow(clippy::borrowed_box)]
  pub fn top(&self) -> Option<&Box<dyn Scene>> {
    self.scenes.last()
  }

  /// Get a mutable reference to the top scene (if any).
  #[allow(clippy::borrowed_box)]
  pub fn top_mut(&mut self) -> Option<&mut Box<dyn Scene>> {
    self.scenes.last_mut()
  }

  /// Get a debug representation of the current scene stack.
  ///
  /// Useful for logging and debugging scene flow.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// println!("{}", manager.debug_stack());
  /// // Output: "[MenuScene] -> [GameplayScene] -> [PauseScene*]"
  /// ```
  pub fn debug_stack(&self) -> String {
    if self.scenes.is_empty() {
      return String::from("(empty)");
    }

    self
      .scenes
      .iter()
      .enumerate()
      .map(|(i, scene)| {
        let name = scene.name();
        if i == self.scenes.len() - 1 {
          format!("[{}*]", name) // Mark active scene
        } else {
          format!("[{}]", name)
        }
      })
      .collect::<Vec<_>>()
      .join(" -> ")
  }

  /// Get the names of all scenes in the stack (bottom to top).
  pub fn scene_names(&self) -> Vec<&str> {
    self.scenes.iter().map(|s| s.name()).collect()
  }

  /// Check if physics should be paused based on scene stack.
  ///
  /// Physics is paused when:
  /// - The top scene explicitly requests it via `is_physics_paused()`
  /// - There's more than one scene and the top scene doesn't allow updates below
  pub fn is_physics_paused(&self) -> bool {
    if let Some(top) = self.scenes.last() {
      // Check if the top scene explicitly requests physics pause
      if top.is_physics_paused() {
        return true;
      }
      // Also pause if there are stacked scenes and top doesn't update below
      if self.scenes.len() > 1 && !top.updates_below() {
        return true;
      }
    }
    false
  }
}

impl Default for SceneManager {
  fn default() -> Self {
    Self::new()
  }
}

// ============================================================================
// Game Trait Adapter
// ============================================================================

use super::game::{Game, GameContext};

/// Adapter that allows a SceneManager to be used with the existing Engine.
///
/// This bridges the scene-based architecture with the existing `Game` trait,
/// allowing you to use scenes without modifying engine internals.
///
/// # Example
///
/// ```rust,ignore
/// use ryo2d::prelude::*;
///
/// // Create your scenes
/// struct MenuScene;
/// impl Scene for MenuScene {
///     fn update(&mut self, ctx: &mut SceneContext, dt: f32) -> SceneAction {
///         if ctx.input.key_pressed(KeyCode::Enter) {
///             SceneAction::Push(Box::new(GameScene::new()))
///         } else {
///             SceneAction::Continue
///         }
///     }
/// }
///
/// // Use SceneGame adapter with the engine
/// let scenes = SceneGame::new(Box::new(MenuScene));
///
/// EngineBuilder::new()
///     .title("My Scene Game")
///     .with_game(scenes)
///     .build()
///     .run()
///     .unwrap();
/// ```
pub struct SceneGame {
  manager: SceneManager,
  /// Whether the initial scene has been entered
  initialized: bool,
  /// Whether a scene requested to quit
  quit_requested: bool,
  /// Counter for generating unique scope IDs
  next_scope_id: u64,
  /// Current scene's scope ID (for command handler cleanup)
  current_scope_id: u64,
}

impl SceneGame {
  /// Create a new SceneGame with an initial scene.
  pub fn new(initial_scene: Box<dyn Scene>) -> Self {
    SceneGame {
      manager: SceneManager::with_scene(initial_scene),
      initialized: false,
      quit_requested: false,
      next_scope_id: 1, // Start at 1 (0 is reserved for game-level permanent scope)
      current_scope_id: 0, // Will be set on first on_enter
    }
  }

  /// Get a reference to the underlying scene manager.
  pub fn manager(&self) -> &SceneManager {
    &self.manager
  }

  /// Get a mutable reference to the underlying scene manager.
  pub fn manager_mut(&mut self) -> &mut SceneManager {
    &mut self.manager
  }
}

impl Game for SceneGame {
  fn init(&mut self, ctx: &mut GameContext) {
    // Assign scope ID for the initial scene
    self.current_scope_id = self.next_scope_id;
    self.next_scope_id += 1;

    // Convert GameContext to SceneContext and call on_enter for initial scene
    let mut scene_ctx = SceneContext {
      world: ctx.world,
      camera: ctx.camera,
      input: ctx.input,
      assets: ctx.assets,
      audio: ctx.audio,
      effects: ctx.effects,
      lighting: ctx.lighting,
      events: ctx.events,
      window_request: ctx.window_request,
      cursor_request: ctx.cursor_request,
      time_control: ctx.time_control,
      profiling_enabled: ctx.profiling_enabled,
      scale_factor: ctx.scale_factor,
      command_registry: ctx.command_registry,
      scope_id: self.current_scope_id,
    };

    // Call on_enter for the initial scene
    if let Some(scene) = self.manager.top_mut() {
      #[cfg(debug_assertions)]
      let start = if scene_ctx.profiling_enabled {
        Some(std::time::Instant::now())
      } else {
        None
      };

      scene.on_enter(&mut scene_ctx);

      #[cfg(debug_assertions)]
      if let Some(start) = start {
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 5 {
          eprintln!("[Scene] initial on_enter took {:?}", elapsed);
        }
      }
    }
    self.initialized = true;
  }

  fn update(&mut self, ctx: &mut GameContext, dt: f32) {
    // Convert GameContext to SceneContext
    let mut scene_ctx = SceneContext {
      world: ctx.world,
      camera: ctx.camera,
      input: ctx.input,
      assets: ctx.assets,
      audio: ctx.audio,
      effects: ctx.effects,
      lighting: ctx.lighting,
      events: ctx.events,
      window_request: ctx.window_request,
      cursor_request: ctx.cursor_request,
      time_control: ctx.time_control,
      profiling_enabled: ctx.profiling_enabled,
      scale_factor: ctx.scale_factor,
      command_registry: ctx.command_registry,
      scope_id: self.current_scope_id,
    };

    // Update any active transitions first
    self.manager.update_transition(dt, &mut scene_ctx);

    // Only update scenes if not in the middle of a transition
    if !self.manager.is_transitioning() {
      let action = self.manager.update(&mut scene_ctx, dt);

      // Check if this action will change scenes (requires scope cleanup)
      let will_change_scene = matches!(
        action,
        SceneAction::Pop | SceneAction::Switch(_) | SceneAction::Reset(_) |
        SceneAction::PopWith(_) | SceneAction::SwitchWith(_, _) | SceneAction::ResetWith(_, _)
      );

      // Remember old scope if scene will change
      let old_scope_id = if will_change_scene {
        Some(self.current_scope_id)
      } else {
        None
      };

      // Generate new scope for incoming scene
      if will_change_scene {
        self.current_scope_id = self.next_scope_id;
        self.next_scope_id += 1;
        // Update context with new scope
        scene_ctx.scope_id = self.current_scope_id;
      }

      let should_continue = self.manager.handle_action(action, &mut scene_ctx);

      // Clean up old scope's command handlers
      if let Some(old_scope) = old_scope_id {
        scene_ctx.command_registry.clear_scope(old_scope);
      }

      if !should_continue {
        self.quit_requested = true;
      }
    }
  }

  fn get_overlay(&self) -> Option<(f32, [u8; 4])> {
    let alpha = self.manager.transition_overlay_alpha();
    if alpha > 0.001 {
      let color = self
        .manager
        .transition_overlay_color()
        .unwrap_or([0, 0, 0, 255]);
      Some((alpha, color))
    } else {
      None
    }
  }

  fn render_ui(&mut self, ui: &mut UiContext) {
    self.manager.render_ui(ui);
  }

  fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
    self.manager.render_background(frame, width, height, camera);
  }

  fn render_gpu(&mut self, ctx: &mut super::game::GpuRenderContext) {
    self.manager.render_gpu(ctx);
  }

  fn is_physics_paused(&self) -> bool {
    // Physics is paused when the top scene doesn't update below
    // (e.g., a pause menu that blocks the game scene)
    self.manager.is_physics_paused()
  }

  fn should_quit(&self) -> bool {
    self.quit_requested
  }
}

impl SceneGame {
  /// Check if a transition is currently active.
  pub fn is_transitioning(&self) -> bool {
    self.manager.is_transitioning()
  }

  /// Get the transition overlay alpha for rendering fade effects.
  ///
  /// Returns a value from 0.0 (fully transparent) to 1.0 (fully opaque).
  pub fn transition_overlay(&self) -> (f32, [u8; 4]) {
    let alpha = self.manager.transition_overlay_alpha();
    let color = self
      .manager
      .transition_overlay_color()
      .unwrap_or([0, 0, 0, 255]);
    (alpha, color)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  // Mock scene for testing
  struct TestScene {
    #[allow(dead_code)]
    name: String,
    entered: bool,
    exited: bool,
    paused: bool,
    resumed: bool,
    update_count: u32,
  }

  impl TestScene {
    fn new(name: &str) -> Self {
      TestScene {
        name: name.to_string(),
        entered: false,
        exited: false,
        paused: false,
        resumed: false,
        update_count: 0,
      }
    }
  }

  impl Scene for TestScene {
    fn on_enter(&mut self, _ctx: &mut SceneContext) {
      self.entered = true;
    }

    fn on_exit(&mut self, _ctx: &mut SceneContext) {
      self.exited = true;
    }

    fn on_pause(&mut self, _ctx: &mut SceneContext) {
      self.paused = true;
    }

    fn on_resume(&mut self, _ctx: &mut SceneContext) {
      self.resumed = true;
    }

    fn update(&mut self, _ctx: &mut SceneContext, _dt: f32) -> SceneAction {
      self.update_count += 1;
      SceneAction::Continue
    }
  }

  #[test]
  fn scene_manager_push_pop() {
    // We can't easily test with a real context, so just verify the struct works
    let manager = SceneManager::new();
    assert!(manager.is_empty());
    assert_eq!(manager.len(), 0);
  }

  #[test]
  fn scene_manager_with_scene() {
    let manager = SceneManager::with_scene(Box::new(TestScene::new("initial")));
    assert!(!manager.is_empty());
    assert_eq!(manager.len(), 1);
  }

  #[test]
  fn scene_action_default() {
    let action: SceneAction = Default::default();
    assert!(matches!(action, SceneAction::Continue));
  }

  #[test]
  fn test_scene_lifecycle_flags() {
    let scene = TestScene::new("test");
    assert!(!scene.entered);
    assert!(!scene.exited);
    assert!(!scene.paused);
    assert!(!scene.resumed);
    assert_eq!(scene.update_count, 0);
  }

  #[test]
  fn transition_none_default() {
    let transition: Transition = Default::default();
    assert!(matches!(transition, Transition::None));
    assert_eq!(transition.duration(), Duration::ZERO);
  }

  #[test]
  fn transition_fade_black() {
    let transition = Transition::fade(Duration::from_millis(500));
    match transition {
      Transition::Fade { duration, color } => {
        assert_eq!(duration, Duration::from_millis(500));
        assert_eq!(color, [0, 0, 0, 255]);
      }
      _ => panic!("Expected Fade transition"),
    }
    assert_eq!(transition.duration(), Duration::from_millis(1000)); // 2x (out + in)
  }

  #[test]
  fn transition_fade_white() {
    let transition = Transition::fade_white(Duration::from_millis(300));
    match transition {
      Transition::Fade { color, .. } => {
        assert_eq!(color, [255, 255, 255, 255]);
      }
      _ => panic!("Expected Fade transition"),
    }
  }

  #[test]
  fn transition_fade_custom_color() {
    let transition = Transition::fade_color(Duration::from_millis(200), 128, 64, 32);
    match transition {
      Transition::Fade { color, .. } => {
        assert_eq!(color, [128, 64, 32, 255]);
      }
      _ => panic!("Expected Fade transition"),
    }
  }

  #[test]
  fn transition_slide() {
    let transition = Transition::slide(Duration::from_millis(400), SlideDirection::Left);
    match transition {
      Transition::Slide {
        duration,
        direction,
      } => {
        assert_eq!(duration, Duration::from_millis(400));
        assert_eq!(direction, SlideDirection::Left);
      }
      _ => panic!("Expected Slide transition"),
    }
    assert_eq!(transition.duration(), Duration::from_millis(400));
  }

  #[test]
  fn slide_directions() {
    assert_eq!(SlideDirection::Left, SlideDirection::Left);
    assert_ne!(SlideDirection::Left, SlideDirection::Right);
    assert_ne!(SlideDirection::Up, SlideDirection::Down);
  }

  #[test]
  fn scene_manager_empty() {
    let manager = SceneManager::new();
    assert!(manager.is_empty());
    assert_eq!(manager.len(), 0);
    assert!(!manager.is_transitioning());
    assert_eq!(manager.transition_progress(), None);
  }

  #[test]
  fn scene_manager_clone_transition() {
    let transition1 = Transition::fade(Duration::from_millis(100));
    let transition2 = transition1.clone();

    match (transition1, transition2) {
      (Transition::Fade { duration: d1, .. }, Transition::Fade { duration: d2, .. }) => {
        assert_eq!(d1, d2);
      }
      _ => panic!("Clone didn't preserve transition type"),
    }
  }

  #[test]
  fn transition_overlay_alpha_no_transition() {
    let manager = SceneManager::new();
    assert_eq!(manager.transition_overlay_alpha(), 0.0);
    assert_eq!(manager.transition_overlay_color(), None);
  }

  #[test]
  fn scene_name_defaults_to_type_name() {
    let scene = TestScene::new("test");
    assert_eq!(scene.name(), "TestScene");
  }

  #[test]
  fn scene_transparent_defaults_to_false() {
    let scene = TestScene::new("test");
    assert!(!scene.is_transparent());
  }

  #[test]
  fn scene_updates_below_defaults_to_false() {
    let scene = TestScene::new("test");
    assert!(!scene.updates_below());
  }
}
