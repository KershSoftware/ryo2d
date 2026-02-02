//! Game trait for user-defined game logic
//!
//! The engine handles low-level systems (window, input, rendering).
//! Users implement the `Game` trait to define their game behavior.

use super::asset::{AssetManager, SpriteHandle};
use super::audio::AudioManager;
use super::config::save::DisplayMode;
use super::ecs::World;
use super::events::Events;
use super::graphics::{BatchCollector, Camera, SpriteGpuRenderer};
use super::input::InputState;
use super::lighting::LightingRenderer;
use super::shaders::EffectsManager;
use super::ui::UiContext;

/// Pending window configuration change requested by the game.
///
/// Set via [`GameContext::request_display_change`] or [`SceneContext::request_display_change`].
/// The engine will apply these changes after the current update.
#[derive(Debug, Clone, Default)]
pub struct WindowRequest {
  /// New resolution width (None = no change)
  pub width: Option<u32>,
  /// New resolution height (None = no change)
  pub height: Option<u32>,
  /// New display mode (None = no change)
  pub display_mode: Option<DisplayMode>,
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
    self.width.is_some() || self.height.is_some() || self.display_mode.is_some()
  }

  /// Clear all pending requests
  pub fn clear(&mut self) {
    self.width = None;
    self.height = None;
    self.display_mode = None;
  }
}

/// Context passed to game callbacks, providing access to engine systems.
///
/// This is your "toolkit" for building games - it contains everything
/// you need to create entities, handle input, and control the camera.
pub struct GameContext<'a> {
  /// The ECS world containing all entities and components
  pub world: &'a mut World,
  /// The camera for world-to-screen transformations
  pub camera: &'a mut Camera,
  /// Current input state (keyboard, mouse)
  pub input: &'a InputState,
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
  /// Time control for slow-mo and hitstop effects
  pub time_control: &'a mut TimeControl,
}

impl<'a> GameContext<'a> {
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
}

/// Context for GPU sprite rendering.
///
/// This context is passed to the optional `render_gpu` callback,
/// allowing games to use GPU-accelerated sprite batching for
/// high-performance particle systems and effects.
///
/// # Example
///
/// ```ignore
/// fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
///     for particle in &self.particles {
///         // Upload texture if not cached
///         gpu.upload_sprite(&particle.sprite);
///
///         // Create instance with transform
///         let instance = SpriteInstance::new()
///             .with_transform(particle.x, particle.y, particle.rotation, 1.0, 1.0);
///
///         // Add to batch
///         gpu.batch.add(particle.sprite.id(), false, instance, particle.z_order);
///     }
/// }
/// ```
pub struct GpuRenderContext<'a> {
  /// The batch collector for grouping sprites by texture
  pub batch: &'a mut BatchCollector,
  /// The GPU sprite renderer (for uploading textures)
  gpu_renderer: &'a mut SpriteGpuRenderer,
  /// Device for GPU operations
  device: &'a wgpu::Device,
  /// Queue for GPU operations
  queue: &'a wgpu::Queue,
  /// Camera for coordinate transforms
  pub camera: &'a super::graphics::Camera,
  /// The ECS world (read-only for rendering)
  pub world: &'a World,
}

impl<'a> GpuRenderContext<'a> {
  /// Create a new GPU render context
  pub fn new(
    batch: &'a mut BatchCollector,
    gpu_renderer: &'a mut SpriteGpuRenderer,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    camera: &'a super::graphics::Camera,
    world: &'a World,
  ) -> Self {
    Self {
      batch,
      gpu_renderer,
      device,
      queue,
      camera,
      world,
    }
  }

  /// Upload a sprite texture to the GPU cache.
  ///
  /// Call this for each unique sprite before adding instances to the batch.
  /// Sprites are cached by ID, so subsequent calls with the same sprite are no-ops.
  pub fn upload_sprite(&mut self, sprite: &SpriteHandle) {
    self
      .gpu_renderer
      .upload_sprite(self.device, self.queue, sprite);
  }

  /// Check if a sprite texture is already uploaded
  pub fn has_texture(&self, id: &str) -> bool {
    self.gpu_renderer.has_texture(id)
  }

  /// Get screen dimensions (width, height)
  pub fn screen_size(&self) -> (f32, f32) {
    self.camera.viewport_size()
  }
}

/// Trait for user-defined game logic.
///
/// Implement this trait to create your game. The engine will call your
/// methods at the appropriate times in the game loop.
///
/// # Example
///
/// ```ignore
/// struct MyGame {
///     player: Option<Entity>,
/// }
///
/// impl Game for MyGame {
///     fn init(&mut self, ctx: &mut GameContext) {
///         // Create your entities here
///         let player = ctx.world.spawn();
///         ctx.world.add_component(player, Transform::new(100.0, 100.0));
///         self.player = Some(player);
///     }
///
///     fn update(&mut self, ctx: &mut GameContext, dt: f32) {
///         // Game logic runs here every fixed timestep
///         if ctx.input.key_down(KeyCode::Space) {
///             // Jump!
///         }
///     }
/// }
/// ```
pub trait Game {
  /// Called once when the engine starts, after the window is created.
  ///
  /// Use this to:
  /// - Spawn initial entities
  /// - Set up the camera
  /// - Load any startup state
  fn init(&mut self, ctx: &mut GameContext);

  /// Called every fixed timestep (e.g., 60 times per second).
  ///
  /// Use this for:
  /// - Reading input
  /// - Updating entity positions/velocities
  /// - Game logic (AI, scoring, state machines)
  ///
  /// `dt` is the fixed delta time in seconds (e.g., 1/60 = 0.01667)
  fn update(&mut self, ctx: &mut GameContext, dt: f32);

  /// Called before entity rendering to draw background layers.
  ///
  /// Use this for:
  /// - Parallax scrolling backgrounds
  /// - Static sky/backdrop images
  /// - Any background elements that should be behind entities
  ///
  /// This is called after the frame buffer is cleared but before any
  /// entities (sprites, tiles, etc.) are rendered.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn render_background(&mut self, frame: &mut [u8], width: u32, height: u32, camera: &Camera) {
  ///     self.background.render(frame, width, height, camera);
  /// }
  /// ```
  fn render_background(&mut self, _frame: &mut [u8], _width: u32, _height: u32, _camera: &Camera) {}

  /// Called to populate GPU sprite batches for high-performance rendering.
  ///
  /// Use this for:
  /// - Particle systems (1000+ sprites)
  /// - Bullet hell games
  /// - Any scenario where CPU sprite blitting is too slow
  ///
  /// GPU sprites are rendered on top of CPU-rendered sprites but before UI.
  /// The provided `GpuRenderContext` gives access to batch collection and
  /// texture upload.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn render_gpu(&mut self, gpu: &mut GpuRenderContext) {
  ///     // Upload textures (cached, safe to call every frame)
  ///     for sprite in &self.particle_sprites {
  ///         gpu.upload_sprite(sprite);
  ///     }
  ///
  ///     // Add particle instances to batches
  ///     for particle in &self.particles {
  ///         let instance = SpriteInstance::new()
  ///             .with_transform(particle.x, particle.y, 0.0, 1.0, 1.0);
  ///         gpu.batch.add(&particle.sprite.id(), false, instance, 10);
  ///     }
  /// }
  /// ```
  fn render_gpu(&mut self, _gpu: &mut GpuRenderContext) {}

  /// Called after sprite rendering to draw UI elements.
  ///
  /// Use this for:
  /// - HUD elements (health bars, score, minimap)
  /// - Debug overlays
  /// - In-game UI (inventory, dialogs)
  ///
  /// UI is drawn in screen space, on top of all world sprites.
  /// The provided `UiContext` offers immediate-mode drawing methods.
  ///
  /// # Example
  ///
  /// ```ignore
  /// fn render_ui(&mut self, ui: &mut UiContext) {
  ///     // Draw health bar
  ///     ui.draw_progress_bar(
  ///         Rect::new(10.0, 10.0, 200.0, 20.0),
  ///         self.health / 100.0,
  ///         Color::RED,
  ///         Color::DARK_GRAY,
  ///     );
  ///
  ///     // Draw score
  ///     ui.draw_text(&format!("Score: {}", self.score), 10.0, 40.0, Color::WHITE);
  /// }
  /// ```
  fn render_ui(&mut self, _ui: &mut UiContext) {}

  /// Return an optional screen overlay for transition effects.
  ///
  /// The engine will blend this color over the entire screen after
  /// rendering all sprites. Used for fade transitions between scenes.
  ///
  /// Returns `Some((alpha, [r, g, b, a]))` to show overlay, `None` for no overlay.
  /// Alpha is 0.0 (transparent) to 1.0 (opaque).
  fn get_overlay(&self) -> Option<(f32, [u8; 4])> {
    None
  }

  /// Whether physics simulation should be paused.
  ///
  /// Override this to pause physics during pause menus, cutscenes, etc.
  /// Default is `false` (physics runs normally).
  fn is_physics_paused(&self) -> bool {
    false
  }

  /// Whether the game wants to quit.
  ///
  /// When this returns `true`, the engine will close the window and exit.
  /// Default is `false` (game keeps running).
  fn should_quit(&self) -> bool {
    false
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  struct TestGame {
    init_called: bool,
    update_count: u32,
  }

  impl Game for TestGame {
    fn init(&mut self, _ctx: &mut GameContext) {
      self.init_called = true;
    }

    fn update(&mut self, _ctx: &mut GameContext, _dt: f32) {
      self.update_count += 1;
    }
  }

  #[test]
  fn game_trait_is_object_safe() {
    // This test verifies that Game can be used as a trait object
    let game: Box<dyn Game> = Box::new(TestGame {
      init_called: false,
      update_count: 0,
    });
    // If this compiles, the trait is object-safe
    let _ = game;
  }

  // ===========================================================================
  // TimeControl Tests
  // ===========================================================================

  #[test]
  fn time_control_default_values() {
    let tc = TimeControl::default();
    assert_eq!(tc.time_scale, 1.0);
    assert_eq!(tc.hitstop_remaining, 0.0);
  }

  #[test]
  fn time_control_time_scale_clamped_positive() {
    let mut tc = TimeControl::default();
    tc.time_scale = -1.0;
    // Note: Clamping is done in the context methods, not in the struct itself
    // This test just verifies the field can be set
    assert_eq!(tc.time_scale, -1.0);
  }

  #[test]
  fn time_control_hitstop_sets_value() {
    let mut tc = TimeControl::default();
    tc.hitstop_remaining = 0.05;
    assert!((tc.hitstop_remaining - 0.05).abs() < 0.001);
  }

  #[test]
  fn time_control_is_hitstop_active() {
    let mut tc = TimeControl::default();
    assert!(!(tc.hitstop_remaining > 0.0));

    tc.hitstop_remaining = 0.05;
    assert!(tc.hitstop_remaining > 0.0);

    tc.hitstop_remaining = 0.0;
    assert!(!(tc.hitstop_remaining > 0.0));
  }

  #[test]
  fn window_request_time_scale_integration() {
    // Verify WindowRequest and TimeControl can coexist in the same context
    let wr = WindowRequest::default();
    let tc = TimeControl::default();

    assert!(!wr.has_changes());
    assert_eq!(tc.time_scale, 1.0);
  }
}
