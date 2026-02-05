use std::sync::Arc;

use gilrs::Gilrs;
use winit::application::ApplicationHandler;
use winit::event::{Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use super::asset::AssetManager;
use super::audio::AudioManager;
use super::config::{
  self as debug, DebugConfig, DebugStats, DisplayMode, FrameTimings, OverlayMode,
  SmoothedDebugStats, SmoothedFrameTimings,
};
use super::console::{
  CommandContext, CommandRegistry, CommandResult, ConsoleState, register_builtins,
};
use super::ecs::{
  Entity, PreviousTransform, RenderStyle, Renderable, SpriteShader, Transform, World,
  WorldTransform, YSort,
};
use super::error::{EngineError, EngineResult};
use super::events::{CollisionStarted, CollisionStopped, Events};
use super::game::{Game, GameContext, GpuRenderContext, TimeControl, TimeInfo, WindowRequest};
use super::graphics::{
  BatchCollector, Camera, CompositeTile, Sprite, SpriteGpuRenderer, TileDirection, WgpuRenderer,
};
use super::input::InputState;
use super::lighting::{Light2D, LightOccluder2D, LightingRenderer, generate_shadow_geometry};
use super::physics::{PhysicsConfig, RapierPhysicsWorld};
use super::scene::{Scene, SceneGame};
use super::shaders::EffectsManager;
use super::systems::{
  AnimationSystem, InterpolationSystem, KnockbackSystem, PhysicsSystem, SquashStretchSystem,
  TrailSystem, TweenSystem,
};
use super::timing::TimeState;
use super::trail::{Line2D, Trail2D, TrailRenderer};
use super::ui::{BitmapFont, Color, UiContext, create_debug_font};
use crate::profile_scope;
use crate::{engine_debug, engine_error, engine_info, engine_warn};

/// Window resize behavior.
///
/// Controls how the game viewport responds to window resizing and maximizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowMode {
  /// Fixed window size, no resizing or maximizing.
  ///
  /// The window cannot be resized by the user. Best for games that want
  /// complete control over presentation.
  Fixed,

  /// Resizable with aspect ratio preservation (letterbox/pillarbox).
  ///
  /// The game viewport scales to fit the window while maintaining its original
  /// aspect ratio. Black bars fill unused space. This is the default and most
  /// common approach for 2D games.
  ///
  /// Example: 800×600 game in a 1920×1080 window will scale up with black bars
  /// on the sides.
  #[default]
  ResizableAspectRatio,

  /// Resizable with integer scaling only (pixel-perfect).
  ///
  /// The game viewport scales in integer multiples (1×, 2×, 3×, 4×) to maintain
  /// perfect pixel crispness. Letterboxing fills remaining space. Best for
  /// retro-styled pixel art games.
  ///
  /// Example: 800×600 game in a 1000×800 window will scale to 1× (800×600) with
  /// black bars, not to fit the full window.
  ResizableIntegerScale,

  /// Resizable, stretch to fill (may distort aspect ratio).
  ///
  /// The game viewport stretches to fill the entire window, potentially distorting
  /// the aspect ratio. Rarely used as a default, but can be useful for UI-heavy
  /// games where distortion isn't noticeable.
  ResizableStretch,
}

/// Configuration for the engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
  pub title: String,
  pub width: u32,
  pub height: u32,
  pub target_fps: u32,
  pub physics_hz: u32,
  pub physics: PhysicsConfig,
  pub debug: DebugConfig,
  pub window_mode: WindowMode,
  /// Display mode (windowed, fullscreen, borderless)
  pub display_mode: DisplayMode,
  /// Maximum width for letterboxed viewport (None = no limit)
  pub max_width: Option<u32>,
  /// Maximum height for letterboxed viewport (None = no limit)
  pub max_height: Option<u32>,
  /// Path to window icon (PNG). If None, uses embedded Ryo2D logo.
  pub icon_path: Option<std::path::PathBuf>,
  /// UI scale factor for HiDPI displays.
  /// - `None` = auto-detect from DPI (rounds scale_factor to nearest integer)
  /// - `Some(1)` = normal size (default for standard displays)
  /// - `Some(2)` = double size (good for HiDPI/Retina)
  /// - etc.
  pub ui_scale: Option<u32>,
}

/// Re-export for backward compatibility
pub type EngineDebugConfig = DebugConfig;

impl Default for EngineConfig {
  fn default() -> Self {
    EngineConfig {
      title: String::from("Ryo2D"),
      width: 1280,
      height: 720,
      target_fps: 60,
      physics_hz: 60,
      physics: PhysicsConfig::default(),
      debug: EngineDebugConfig::default(),
      window_mode: WindowMode::default(),
      display_mode: DisplayMode::default(),
      max_width: None,
      max_height: None,
      icon_path: None,
      ui_scale: None, // Auto-detect from DPI
    }
  }
}

/// Determines whether manual frame limiting should be applied.
///
/// Frame limiting is applied when:
/// - Presentation doesn't already block (i.e., not using Fifo/VSync mode)
/// - VSync is not explicitly disabled (vsync=false means uncapped FPS)
///
/// # Arguments
/// * `presentation_blocks` - Whether the GPU presentation blocks until vsync (Fifo mode)
/// * `vsync` - VSync preference: None (default), Some(true) (on), Some(false) (off/uncapped)
///
/// # Returns
/// `true` if manual frame limiting should be applied, `false` otherwise
fn should_apply_frame_limiting(presentation_blocks: bool, vsync: Option<bool>) -> bool {
  // Don't limit if presentation already blocks (Fifo mode handles timing)
  if presentation_blocks {
    return false;
  }

  // Don't limit if vsync is explicitly disabled (user wants uncapped FPS)
  if vsync == Some(false) {
    return false;
  }

  // Apply frame limiting for all other cases
  true
}

/// The main engine struct
pub struct Engine {
  config: EngineConfig,
  window: Option<Arc<Window>>,
  renderer: Option<WgpuRenderer>,
  time: Option<TimeState>,
  input: InputState,
  // ECS World
  world: World,
  // Camera for world-to-screen transformations
  camera: Camera,
  // Asset manager for loading and caching resources
  assets: AssetManager,
  // Audio manager for sound effects and music
  audio: Option<AudioManager>,
  // Post-process effects manager
  effects: EffectsManager,
  // User-defined game logic (Option to allow taking ownership temporarily)
  game: Option<Box<dyn Game>>,
  // Whether init has been called
  initialized: bool,
  // Whether the first frame has been rendered (used to show window)
  first_frame_rendered: bool,
  // Cached sorted list of Renderable component entities
  renderable_cache: Vec<(Entity, i32)>,
  // Whether render cache needs rebuilding (entities spawned/despawned, z_order changed)
  render_cache_dirty: bool,
  // Cached list of entities with YSort component (for per-frame Y-based sorting)
  y_sort_entities: Vec<Entity>,
  // Rapier2D physics world
  physics: RapierPhysicsWorld,
  // UI font for text rendering
  ui_font: BitmapFont,
  // Last known world generation (for detecting world.clear() calls)
  last_world_generation: u64,
  // Cached window size to avoid redundant resize calls
  last_window_size: (u32, u32),
  // VSync preference from system.json (None = use engine default)
  vsync: Option<bool>,
  // GPU sprite renderer for entities with Material/shader effects
  sprite_gpu: Option<SpriteGpuRenderer>,
  // Batch collector for GPU sprite rendering (reused each frame)
  batch_collector: BatchCollector,
  // 2D Lighting renderer
  lighting: Option<LightingRenderer>,
  // Trail/line renderer for motion trails and procedural lines
  trail_renderer: Option<TrailRenderer>,
  // Frame timing breakdown for profiling (current frame, in-progress)
  frame_timings: FrameTimings,
  // Smoothed frame timings for stable overlay display
  smoothed_timings: SmoothedFrameTimings,
  // Current overlay mode (controlled by F1 key)
  overlay_mode: OverlayMode,
  // Debug statistics for current frame
  debug_stats: DebugStats,
  // Smoothed debug stats for stable overlay display
  smoothed_stats: SmoothedDebugStats,
  // Whether debug keys (F1-F4) are enabled
  debug_keys_enabled: bool,
  // Whether debug font has been uploaded to GPU
  debug_font_uploaded: bool,
  // Temporary buffer for UI when not affected by lighting
  ui_buffer: Vec<u8>,
  // Pending window configuration changes from game
  window_request: WindowRequest,
  // Pending cursor configuration changes from game
  cursor_request: crate::core::cursor::CursorRequest,
  // Cache of custom cursors by their ID (sprite path)
  custom_cursor_cache: std::collections::HashMap<String, winit::window::CustomCursor>,
  // Flag indicating surface needs reconfiguration (after display mode change)
  surface_needs_reconfigure: bool,
  // Event bus for decoupled inter-system communication
  events: Events,
  // Gamepad input library (gilrs)
  gilrs: Option<Gilrs>,
  // Time control for slow-mo and hitstop effects (shared with GameContext/SceneContext)
  time_control: TimeControl,
  // Profiler window (only when profiler feature is enabled)
  #[cfg(feature = "profiler")]
  profiler_window: Option<super::windows::profiler_window::ProfilerWindow>,
  // Logger window for viewing runtime logs
  logger_window: Option<super::windows::logger_window::LoggerWindow>,
  // Track focus state of each window separately (game throttles only when ALL lose focus)
  game_window_focused: bool,
  #[cfg(feature = "profiler")]
  profiler_window_focused: bool,
  logger_window_focused: bool,
  // Pending requests to open dev windows (set by F9/F10, processed in event loop)
  #[cfg(feature = "profiler")]
  pending_open_profiler: bool,
  pending_open_logger: bool,
  // Pending request to toggle borderless window (set by F5, processed in event loop)
  pending_toggle_borderless: bool,
  // Current window scale factor (DPI scaling)
  scale_factor: f64,
  // Computed UI scale factor (from config or auto-detected from DPI)
  ui_scale: u32,
  // Developer console state
  console_state: ConsoleState,
  // Console command registry
  command_registry: CommandRegistry,
  // Whether console requested quit
  console_quit_requested: bool,
  // Game name (from Ryo.toml [project] section)
  game_name: Option<String>,
  // Game version (from Ryo.toml [project] section)
  game_version: Option<String>,
}

impl Engine {
  // =============================================================================
  // Coordinate Transformations & Utilities
  // =============================================================================

  /// Transform window coordinates to game coordinates.
  ///
  /// Handles scaling and letterboxing when window size differs from game resolution.
  /// Returns `None` if the coordinates are outside the game viewport (in black bars).
  fn window_to_game_coords(&self, window_x: f64, window_y: f64) -> Option<(f32, f32)> {
    if let (Some(window), Some(_renderer)) = (&self.window, &self.renderer) {
      let window_size = window.inner_size();
      let (game_width, game_height) = (self.config.width as f32, self.config.height as f32);

      // For stretch mode, coordinates map 1:1 to the resized buffer
      if self.config.window_mode == WindowMode::ResizableStretch {
        return Some((window_x as f32, window_y as f32));
      }

      // Calculate scaling and letterboxing for aspect ratio preservation
      let window_aspect = window_size.width as f32 / window_size.height as f32;
      let game_aspect = game_width / game_height;

      let mut scale = if window_aspect > game_aspect {
        // Window is wider - pillarboxing (black bars on sides)
        window_size.height as f32 / game_height
      } else {
        // Window is taller - letterboxing (black bars on top/bottom)
        window_size.width as f32 / game_width
      };

      // Apply max width/height constraints if set
      if let Some(max_width) = self.config.max_width {
        let max_scale_width = max_width as f32 / game_width;
        scale = scale.min(max_scale_width);
      }
      if let Some(max_height) = self.config.max_height {
        let max_scale_height = max_height as f32 / game_height;
        scale = scale.min(max_scale_height);
      }

      // For integer scale mode, round down the scale
      let scale = if self.config.window_mode == WindowMode::ResizableIntegerScale {
        scale.floor().max(1.0)
      } else {
        scale
      };

      // Recalculate offsets with constrained scale
      let scaled_width = game_width * scale;
      let scaled_height = game_height * scale;
      let offset_x = (window_size.width as f32 - scaled_width) / 2.0;
      let offset_y = (window_size.height as f32 - scaled_height) / 2.0;

      // Transform from window space to game space
      let game_x = (window_x as f32 - offset_x) / scale;
      let game_y = (window_y as f32 - offset_y) / scale;

      // Check if coordinates are within game bounds (reject clicks in black bars)
      if game_x < 0.0 || game_x > game_width || game_y < 0.0 || game_y > game_height {
        return None;
      }

      Some((game_x, game_y))
    } else {
      // Fallback if window/renderer not initialized
      Some((window_x as f32, window_y as f32))
    }
  }

  /// Apply pending window configuration changes from the game.
  fn apply_window_request(&mut self, window: &Arc<Window>) {
    // Apply resolution change
    if let (Some(width), Some(height)) = (self.window_request.width, self.window_request.height) {
      let new_size = winit::dpi::PhysicalSize::new(width, height);
      if let Some(actual_size) = window.request_inner_size(new_size) {
        // Update internal config
        self.config.width = actual_size.width;
        self.config.height = actual_size.height;
        self.last_window_size = (actual_size.width, actual_size.height);
        // Update camera viewport
        self.camera = Camera::centered_at(width as f32 / 2.0, height as f32 / 2.0, width, height);
      }
    }

    // Apply display mode change
    if let Some(mode) = self.window_request.display_mode {
      match mode {
        DisplayMode::Windowed => {
          window.set_fullscreen(None);
          window.set_decorations(true); // Restore window title bar
        }
        DisplayMode::ExclusiveFullscreen => {
          // Exclusive fullscreen - use current monitor's native resolution
          if let Some(monitor) = window.current_monitor()
            && let Some(video_mode) = monitor.video_modes().next()
          {
            window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(video_mode)));
          }
        }
        DisplayMode::BorderlessFullscreen => {
          // Borderless fullscreen - fill screen without mode change
          window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(
            window.current_monitor(),
          )));
        }
      }
      // Update internal config
      self.config.display_mode = mode;

      // Mark that surface needs reconfiguration on next frame
      self.surface_needs_reconfigure = true;
    }

    // Apply UI scale change
    if let Some(scale) = self.window_request.ui_scale {
      if scale == 0 {
        // Auto-detect mode: use DPI scale factor
        self.ui_scale = (self.scale_factor.round() as u32).max(1);
        self.config.ui_scale = None;
      } else {
        // Manual mode: use specified scale
        self.ui_scale = scale.max(1);
        self.config.ui_scale = Some(scale);
      }
    }

    // Clear the request after processing
    self.window_request.clear();
  }

  /// Apply pending cursor configuration changes from the game.
  fn apply_cursor_request(&mut self, window: &Arc<Window>, event_loop: &ActiveEventLoop) {
    use winit::window::CursorGrabMode;

    // Take ownership of the request to avoid borrow issues
    let request = std::mem::take(&mut self.cursor_request);

    // Custom cursor takes precedence over system cursor
    if let Some(custom_data) = &request.custom_cursor {
      // Check cache first
      if let Some(cached) = self.custom_cursor_cache.get(&custom_data.id) {
        window.set_cursor(cached.clone());
      } else {
        // Create new custom cursor from RGBA data
        match winit::window::CustomCursor::from_rgba(
          custom_data.rgba.clone(),
          custom_data.width,
          custom_data.height,
          custom_data.hotspot_x,
          custom_data.hotspot_y,
        ) {
          Ok(source) => {
            let cursor = event_loop.create_custom_cursor(source);
            window.set_cursor(cursor.clone());
            // Cache for future use
            self
              .custom_cursor_cache
              .insert(custom_data.id.clone(), cursor);
          }
          Err(e) => {
            engine_warn!(module: "Cursor", "Failed to create custom cursor: {:?}", e);
          }
        }
      }
    } else if let Some(style) = &request.cursor {
      // Apply system cursor style
      window.set_cursor(winit::window::Cursor::Icon(style.to_winit()));
    }

    // Apply visibility change
    if let Some(visible) = request.visible {
      window.set_cursor_visible(visible);
    }

    // Apply confinement change
    if let Some(confined) = request.confined {
      let mode = if confined {
        CursorGrabMode::Confined
      } else {
        CursorGrabMode::None
      };
      // Try confined mode, fall back to none if not supported
      if window.set_cursor_grab(mode).is_err() {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
      }
    }
  }

  /// Load a window icon from a file path or use the embedded default logo.
  ///
  /// Returns a winit Icon that can be set on the window.
  fn load_window_icon(icon_path: Option<&std::path::Path>) -> Option<winit::window::Icon> {
    // Try to load custom icon if specified
    if let Some(path) = icon_path {
      if let Ok(img) = image::open(path) {
        let rgba = img.into_rgba8();
        let (width, height) = rgba.dimensions();
        let raw = rgba.into_raw();
        return winit::window::Icon::from_rgba(raw, width, height).ok();
      } else {
        engine_warn!(
          module: "Icon",
          "Failed to load custom icon from {:?}, using default",
          path
        );
      }
    }

    // Fall back to embedded Ryo2D logo
    let logo_bytes = include_bytes!("../assets/logo.png");
    if let Ok(img) = image::load_from_memory(logo_bytes) {
      let rgba = img.into_rgba8();
      let (width, height) = rgba.dimensions();
      let raw = rgba.into_raw();
      return winit::window::Icon::from_rgba(raw, width, height).ok();
    }

    None
  }

  #[allow(clippy::too_many_arguments)]
  fn new(
    config: EngineConfig,
    game: Box<dyn Game>,
    vsync: Option<bool>,
    master_volume: f32,
    music_volume: f32,
    sfx_volume: f32,
    disable_debug_keys: bool,
    game_name: Option<String>,
    game_version: Option<String>,
    console_commands: Vec<(String, String, Option<String>)>,
  ) -> Self {
    let world = World::new();
    let camera = Camera::centered_at(
      config.width as f32 / 2.0,
      config.height as f32 / 2.0,
      config.width,
      config.height,
    );
    #[cfg_attr(not(debug_assertions), allow(unused_mut))]
    let mut assets = AssetManager::new();
    let ui_font = create_debug_font();
    let initial_size = (config.width, config.height);
    let ui_buffer_size = (config.width * config.height * 4) as usize;

    // Enable hot reload in debug builds if a workspace with assets directory exists
    #[cfg(debug_assertions)]
    {
      if let Ok(workspace) = super::config::Workspace::discover() {
        let assets_dir = workspace.assets_dir();
        if assets_dir.exists()
          && let Err(e) = assets.enable_hot_reload(vec![assets_dir])
        {
          engine_warn!(module: "HotReload", "Failed to enable: {}", e);
        }
      }
    }

    // Try to initialize audio (gracefully handle no audio device)
    let mut audio = match AudioManager::new() {
      Ok(manager) => Some(manager),
      Err(e) => {
        engine_warn!(
          module: "Audio",
          "Could not initialize audio: {}. Continuing without sound.",
          e
        );
        None
      }
    };

    // Apply audio volumes from system.json if audio was initialized
    if let Some(ref mut audio_manager) = audio {
      audio_manager.set_master_volume(master_volume);
      audio_manager.set_music_volume(music_volume);
      audio_manager.set_sfx_volume(sfx_volume);
    }

    // Initialize gamepad input (gracefully handle no gamepad support)
    let gilrs = match Gilrs::new() {
      Ok(g) => {
        engine_info!(module: "Gamepad", "Initialized successfully");
        Some(g)
      }
      Err(e) => {
        engine_warn!(
          module: "Gamepad",
          "Could not initialize gamepad support: {}. Continuing without gamepads.",
          e
        );
        None
      }
    };

    // Compute initial overlay mode from config before moving it
    let initial_overlay_mode = OverlayMode::from_flags(
      config.debug.show_fps,
      config.debug.show_frame_timing,
      config.debug.show_stats,
    );

    // Extract ui_scale before moving config
    let initial_ui_scale = config.ui_scale.unwrap_or(1);

    // Initialize developer console with built-in commands
    let console_state = ConsoleState::new();
    let mut command_registry = CommandRegistry::new();
    register_builtins(&mut command_registry);

    // Apply pre-declared console commands from EngineBuilder
    for (name, description, verbose) in console_commands {
      command_registry.declare(name, description, verbose);
    }

    Engine {
      config,
      window: None,
      renderer: None,
      time: None,
      input: InputState::new(),
      world,
      camera,
      assets,
      audio,
      effects: EffectsManager::new(),
      game: Some(game),
      initialized: false,
      first_frame_rendered: false,
      renderable_cache: Vec::new(),
      render_cache_dirty: true,
      y_sort_entities: Vec::new(),
      physics: RapierPhysicsWorld::new(),
      ui_font,
      last_world_generation: 0,
      last_window_size: initial_size,
      vsync,
      sprite_gpu: None, // Initialized when renderer is created
      batch_collector: BatchCollector::new(),
      lighting: None,       // Initialized when renderer is created
      trail_renderer: None, // Initialized when renderer is created
      frame_timings: FrameTimings::default(),
      smoothed_timings: SmoothedFrameTimings::default(),
      overlay_mode: initial_overlay_mode,
      debug_stats: DebugStats::default(),
      smoothed_stats: SmoothedDebugStats::default(),
      debug_keys_enabled: !disable_debug_keys,
      debug_font_uploaded: false,
      ui_buffer: vec![0; ui_buffer_size], // RGBA buffer for UI
      window_request: WindowRequest::default(),
      cursor_request: crate::core::cursor::CursorRequest::default(),
      custom_cursor_cache: std::collections::HashMap::new(),
      surface_needs_reconfigure: false,
      events: Events::new(),
      gilrs,
      time_control: TimeControl::default(),
      #[cfg(feature = "profiler")]
      profiler_window: None, // Initialized in resumed() or on F10 press
      logger_window: None, // Initialized in resumed() or on F9 press
      game_window_focused: true,
      #[cfg(feature = "profiler")]
      profiler_window_focused: false,
      logger_window_focused: false,
      #[cfg(feature = "profiler")]
      pending_open_profiler: false,
      pending_open_logger: false,
      pending_toggle_borderless: false,
      scale_factor: 1.0,          // Updated when window is created
      ui_scale: initial_ui_scale, // Updated when window is created if auto
      console_state,
      command_registry,
      console_quit_requested: false,
      game_name,
      game_version,
    }
  }

  // =============================================================================
  // Main Loop
  // =============================================================================

  /// Run the engine
  pub fn run(mut self) -> EngineResult<()> {
    // Print startup banner to console
    Self::print_startup_banner(&self.config);

    engine_info!(module: "Init", "Initializing {}...", self.config.title);
    engine_info!(module: "Init", "Resolution: {}x{}", self.config.width, self.config.height);
    engine_info!(module: "Init", "Target FPS: {}", self.config.target_fps);

    let event_loop = EventLoop::new().map_err(|e| EngineError::Window(format!("{}", e)))?;

    event_loop
      .run_app(&mut self)
      .map_err(|e| EngineError::Window(format!("Event loop error: {}", e)))
  }

  /// Print startup banner to console with useful info.
  fn print_startup_banner(config: &EngineConfig) {
    let version = env!("CARGO_PKG_VERSION");

    println!();
    println!("  {} | Ryo2D v{}", config.title, version);
    println!(
      "  {}x{} @ {}fps",
      config.width, config.height, config.target_fps
    );
    println!();
    println!("  `      Developer console");
    println!("  F1-F4  Debug overlays");
    println!("  F5     Toggle borderless");
    println!("  F9     Logger");
    #[cfg(feature = "profiler")]
    println!("  F10    Profiler");
    println!();
  }

  /// Run a single frame tick when the game window is not focused.
  /// This keeps the game logic running.
  /// If `throttle` is true, caps to 30 FPS and skips rendering.
  /// If `throttle` is false (profiler has focus), renders at full speed.
  fn tick_frame(&mut self, throttle: bool) {
    // Track frame start for frame limiting
    let frame_start = std::time::Instant::now();

    // Signal profiler that a new frame is starting (needed when profiler has focus)
    #[cfg(feature = "profiler")]
    if !throttle {
      // Track wall-clock frame time for accurate FPS display
      if let Some(ref mut profiler) = self.profiler_window {
        profiler.track_frame_time();
      }
      super::profiler::new_frame();
    }

    // Begin frame timing
    if let Some(time) = &mut self.time {
      time.begin_frame();
    }

    // Process gamepad events even when not focused
    if let Some(ref mut gilrs) = self.gilrs {
      while let Some(_event) = gilrs.next_event() {
        // Just drain events to keep gilrs happy
      }
    }

    // Get real dt for systems that should use unscaled time
    let real_dt = self.time.as_ref().map(|t| t.fixed_dt()).unwrap_or(0.0);

    // Update camera shake with real dt
    self.camera.update_shake(real_dt);

    // Fixed timestep update loop with profile scope
    {
      profile_scope!("cpu:update");
      let mut first_update = true;
      while self
        .time
        .as_ref()
        .map(|t| t.should_update())
        .unwrap_or(false)
      {
        // Sync time_control to TimeState before each update
        if let Some(time) = &mut self.time {
          time.sync_from_time_control(&self.time_control);
        }

        let scaled_dt = self
          .time
          .as_ref()
          .map(|t| t.scaled_fixed_dt())
          .unwrap_or(0.0);
        self.update(scaled_dt);

        if first_update {
          self.input.suppress_oneshots();
          first_update = false;
        }

        if let Some(time) = &mut self.time {
          time.consume_update();
        }
      }
      self.input.unsuppress_oneshots();
    }

    // Decay hitstop and sync back to time_control
    if let Some(time) = &mut self.time {
      time.sync_from_time_control(&self.time_control);
      time.update_hitstop(real_dt);
      time.sync_to_time_control(&mut self.time_control);
    }

    // When profiler has focus (throttle=false), we need to render directly
    // because Windows won't deliver RedrawRequested to unfocused game window
    if !throttle {
      // Do full render and present (same as RedrawRequested handler)
      self.do_full_render_and_present();

      // Print GameLoop debug info (same as RedrawRequested handler)
      if let Some(time) = &mut self.time {
        time.print_debug_info();
      }

      // Frame limit to target FPS (Windows doesn't enforce vsync on unfocused windows)
      let target_frame_time =
        std::time::Duration::from_secs_f64(1.0 / self.config.target_fps as f64);
      let elapsed = frame_start.elapsed();
      if elapsed < target_frame_time {
        std::thread::sleep(target_frame_time - elapsed);
      }

      // Render profiler window AFTER sleep so it reads the correct frame duration
      // (puffin measures from new_frame to new_frame, sleep needs to be included)
      #[cfg(feature = "profiler")]
      if let Some(ref mut profiler) = self.profiler_window {
        profiler.render();
      }

      // Render logger window
      if let Some(ref mut logger) = self.logger_window {
        logger.render();
      }
    } else {
      // Cap to 30 FPS when completely unfocused to reduce CPU usage
      std::thread::sleep(std::time::Duration::from_millis(33));
    }
  }

  /// Full render and present pipeline - handles CPU rendering, GPU sprites, trails, lighting.
  /// Called from both RedrawRequested handler and tick_frame (when profiler has focus).
  fn do_full_render_and_present(&mut self) {
    // Render phase
    {
      profile_scope!("cpu:render");

      // Get interpolation alpha for smooth rendering
      let alpha = self
        .time
        .as_ref()
        .map(|t| t.interpolation_alpha())
        .unwrap_or(0.0);

      // CPU render pass
      self.render(alpha);

      // Call game.render_gpu() to populate GPU sprite batches
      if let Some(mut game) = self.game.take() {
        if let (Some(sprite_gpu), Some(renderer)) =
          (self.sprite_gpu.as_mut(), self.renderer.as_ref())
        {
          // Upload debug font texture on first frame (if debug overlays enabled)
          if !self.debug_font_uploaded
            && (self.config.debug.show_fps
              || self.config.debug.show_frame_timing
              || self.config.debug.show_stats)
          {
            debug::upload_debug_font(
              &self.ui_font,
              sprite_gpu,
              renderer.device(),
              renderer.queue(),
            );
            self.debug_font_uploaded = true;
          }

          let mut gpu_ctx = GpuRenderContext::new(
            &mut self.batch_collector,
            sprite_gpu,
            renderer.device(),
            renderer.queue(),
            &self.camera,
            &self.world,
          );
          game.render_gpu(&mut gpu_ctx);
        }
        self.game = Some(game);
      }

      // Capture debug stats before rendering clears them
      // Count CPU sprites (visible, active entities with Renderable)
      let cpu_sprite_count = self
        .renderable_cache
        .iter()
        .filter(|(entity, _)| {
          !self.world.has::<super::ecs::Inactive>(*entity)
            && self
              .world
              .get_component::<Renderable>(*entity)
              .map(|r| r.visible)
              .unwrap_or(false)
        })
        .count();

      self.debug_stats = DebugStats {
        entity_count: self.world.len(),
        draw_calls: self.batch_collector.batch_count() as u32,
        sprite_count: (cpu_sprite_count + self.batch_collector.sprite_count()) as u32,
      };
    } // render scope ends
    self.smoothed_stats.update(&self.debug_stats);

    // Present phase
    profile_scope!("gpu:present");

    // Take sprite_gpu, lighting, and trail_renderer out to avoid borrow conflicts
    let mut sprite_gpu = self.sprite_gpu.take();
    let mut renderer = self.renderer.take();
    let mut lighting = self.lighting.take();
    let mut trail_renderer = self.trail_renderer.take();

    if let (Some(rend), Some(gpu)) = (&mut renderer, &mut sprite_gpu) {
      // Collect trails and lines from the world (inside renderer block for device/queue access)
      if let Some(ref mut trails) = trail_renderer {
        trails.begin_frame();

        // Add Trail2D components (skip inactive pooled entities)
        for entity in self.world.entities_with::<Trail2D>() {
          if self.world.has::<super::ecs::Inactive>(entity) {
            continue;
          }
          if let (Some(trail), Some(transform)) = (
            self.world.get_component::<Trail2D>(entity),
            self.world.get_component::<Transform>(entity),
          ) {
            // Use add_trail_with_device for texture caching support
            trails.add_trail_with_device(rend.device(), rend.queue(), &trail, &transform);
          }
        }

        // Add Line2D components (skip inactive pooled entities)
        for entity in self.world.entities_with::<Line2D>() {
          if self.world.has::<super::ecs::Inactive>(entity) {
            continue;
          }
          if let (Some(line), Some(transform)) = (
            self.world.get_component::<Line2D>(entity),
            self.world.get_component::<Transform>(entity),
          ) {
            trails.add_line(&line, transform.x, transform.y);
          }
        }
      }
      // Only use GPU sprite callback if we have batched sprites or trails
      let has_gpu_sprites = self.batch_collector.sprite_count() > 0;
      let has_trails = trail_renderer
        .as_ref()
        .map(|t| t.vertex_count() > 0)
        .unwrap_or(false);

      // Collect lights from the world
      let lights: Vec<_> = self
        .world
        .query_mut::<(&Transform, &Light2D)>()
        .into_iter()
        .map(|(t, l)| (t.clone(), l.clone()))
        .collect();

      // Collect occluders for shadow casting
      let occluders: Vec<_> = self
        .world
        .query_mut::<(&Transform, &LightOccluder2D)>()
        .into_iter()
        .map(|(t, o)| (t.clone(), o.clone()))
        .collect();

      // Check if any lights cast shadows and there are occluders
      let has_shadow_casting_lights = lights.iter().any(|(_, l)| l.casts_shadows());
      let should_render_shadows = has_shadow_casting_lights && !occluders.is_empty();

      // Auto-enable lighting when Light2D entities exist
      if !lights.is_empty()
        && let Some(ref mut l) = lighting
        && !l.is_enabled()
      {
        l.set_enabled(true);
      }

      // Update god rays effect with positions from lights that contribute
      if let Some(ref l) = lighting {
        let cam_pos = glam::Vec2::new(self.camera.x, self.camera.y);
        l.update_god_rays(&mut self.effects, &lights, cam_pos, self.camera.zoom);
      }

      // Update GPU renderer viewport
      gpu.set_viewport(
        rend.queue(),
        rend.game_width() as f32,
        rend.game_height() as f32,
      );

      // Prepare camera data for trail rendering
      let camera_pos = glam::Vec2::new(self.camera.x, self.camera.y);
      let camera_zoom = self.camera.zoom;
      let time = self
        .time
        .as_ref()
        .map(|t| t.frame_count() as f32 / 60.0)
        .unwrap_or(0.0);

      // Check if lighting is enabled and should be applied
      let lighting_enabled = lighting.as_ref().map(|l| l.is_enabled()).unwrap_or(false);

      let result = if has_gpu_sprites || has_trails || lighting_enabled {
        // Create GPU sprite callback (backgrounds, parallax - renders BEFORE CPU content)
        let batch_collector = &self.batch_collector;
        let gpu_callback =
          |queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView| {
            if has_gpu_sprites {
              gpu.render_batches(queue, encoder, view, batch_collector);
            }
          };

        // Create trail callback (renders AFTER CPU content so backgrounds don't cover trails)
        let trail_callback =
          |queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView| {
            if let Some(ref mut trails) = trail_renderer {
              trails.render(
                queue,
                encoder,
                view,
                camera_pos,
                camera_zoom,
                time,
                super::trail::TrailStyle::Solid,
              );
            }
          };

        // Create lighting callback if lighting is enabled
        if lighting_enabled {
          let lighting_ref = &mut lighting;
          let lighting_callback =
            move |device: &wgpu::Device,
                  queue: &wgpu::Queue,
                  encoder: &mut wgpu::CommandEncoder,
                  scene_texture: &wgpu::Texture,
                  output_view: &wgpu::TextureView| {
              if let Some(ref mut lighting_renderer) = *lighting_ref {
                // Render lights to the light accumulation buffer
                lighting_renderer.render_lights(
                  device,
                  queue,
                  encoder,
                  lights.iter().map(|(t, l)| (t, l)),
                  camera_pos,
                  camera_zoom,
                );

                // Render shadows to subtract from light buffer
                if should_render_shadows {
                  let occluder_refs: Vec<(&Transform, &LightOccluder2D)> =
                    occluders.iter().map(|(t, o)| (t, o)).collect();
                  let shadow_geometry =
                    generate_shadow_geometry(lights.iter().map(|(t, l)| (t, l)), &occluder_refs);
                  lighting_renderer.render_shadows(device, queue, encoder, &shadow_geometry);
                }

                // Compose the lit scene from game texture
                lighting_renderer.compose_from_texture(
                  device,
                  queue,
                  encoder,
                  scene_texture,
                  output_view,
                  time,
                );
              }
            };

          rend.render_with_lighting(
            Some(&self.effects),
            Some(gpu_callback),
            Some(lighting_callback),
            Some(&self.ui_buffer),
          )
        } else {
          rend.render_with_gpu_sprites(
            Some(&self.effects),
            Some(gpu_callback),
            Some(trail_callback),
            Some(&self.ui_buffer),
          )
        }
      } else {
        rend.render_with_effects(Some(&self.effects), Some(&self.ui_buffer))
      };

      match result {
        Ok(_) => {}
        Err(wgpu::SurfaceError::Outdated) => {
          self.surface_needs_reconfigure = true;
        }
        Err(wgpu::SurfaceError::Lost) => {
          if let Some(ref mut rend) = renderer
            && let Some(window) = &self.window
          {
            let size = window.inner_size();
            rend.resize(size.width, size.height);
          }
        }
        Err(e) => {
          engine_error!(module: "Render", "Render error: {:?}", e);
        }
      }
    }

    // Put everything back
    self.sprite_gpu = sprite_gpu;
    self.renderer = renderer;
    self.lighting = lighting;
    self.trail_renderer = trail_renderer;

    // Track first frame
    if !self.first_frame_rendered {
      self.first_frame_rendered = true;
    }
  }

  // =============================================================================
  // Update System - Game Logic & Physics
  // =============================================================================

  /// Fixed timestep update - called multiple times per frame if needed
  ///
  /// This runs user game logic and built-in engine systems (physics, movement, camera).
  fn update(&mut self, _dt: f32) {
    profile_scope!("cpu:engine_update");

    // --- Event Bus Management ---
    // Clear events from previous frame
    self.events.clear();

    // Emit collision events from the physics system (from previous frame's physics step)
    // These are now available via ctx.events.read::<CollisionStarted>()
    for event in self.physics.get_collision_events() {
      if event.is_started() {
        self.events.emit(CollisionStarted {
          entity_a: event.entity_a,
          entity_b: event.entity_b,
        });
      } else {
        self.events.emit(CollisionStopped {
          entity_a: event.entity_a,
          entity_b: event.entity_b,
        });
      }
    }

    // Process hot reloads (debug builds only)
    #[cfg(debug_assertions)]
    {
      let reloaded = self.assets.process_reloads();
      if reloaded > 0 {
        // Mark render cache dirty since sprites may have changed
        self.render_cache_dirty = true;
      }
    }

    // --- Animation System ---
    AnimationSystem::update(&mut self.world, _dt);

    // --- Tween System ---
    TweenSystem::update(&mut self.world, _dt);

    // --- Knockback System ---
    KnockbackSystem::update(&mut self.world, &mut self.events, _dt);

    // --- SquashStretch System ---
    SquashStretchSystem::update(&mut self.world, &mut self.events, _dt);

    // --- Game Logic Update ---
    // Temporarily take ownership of the game to avoid borrow conflicts
    if let Some(mut game) = self.game.take() {
      // Build TimeInfo from current timing state
      let time_info = TimeInfo {
        fixed_dt: self
          .time
          .as_ref()
          .map(|t| t.fixed_dt())
          .unwrap_or(1.0 / 60.0),
        scaled_dt: _dt,
        real_dt: self.time.as_ref().map(|t| t.dt()).unwrap_or(0.0),
        render_alpha: self
          .time
          .as_ref()
          .map(|t| t.interpolation_alpha())
          .unwrap_or(0.0),
        time_scale: self.time_control.time_scale,
        is_hitstop: self.time_control.hitstop_remaining > 0.0,
        frame_count: self.time.as_ref().map(|t| t.frame_count()).unwrap_or(0),
      };

      // Create context for the game
      let mut ctx = GameContext {
        world: &mut self.world,
        camera: &mut self.camera,
        input: &mut self.input,
        assets: &mut self.assets,
        audio: &mut self.audio,
        effects: &mut self.effects,
        lighting: &mut self.lighting,
        events: &mut self.events,
        window_request: &mut self.window_request,
        cursor_request: &mut self.cursor_request,
        time_control: &mut self.time_control,
        time: time_info,
        scale_factor: self.scale_factor,
        command_registry: &mut self.command_registry,
        scope_id: 0, // Game-level scope (permanent)
      };

      // Call user's update function
      game.update(&mut ctx, _dt);

      // Put the game back
      self.game = Some(game);

      // Mark render cache dirty if any entities were spawned or despawned
      if self.world.take_entities_changed() {
        self.render_cache_dirty = true;
      }
    }

    // --- Hierarchy Transform Propagation ---
    // Update world transforms for all entities with parent-child relationships.
    // This must happen after user update (so local transforms are set) and
    // before rendering (so WorldTransform is current).
    self.world.propagate_transforms();

    // --- Trail2D System ---
    TrailSystem::update(&mut self.world, _dt);

    // --- Post-Process Effects ---
    // Update timed post-process effects
    self.effects.update(_dt);

    // --- Physics System (Rapier2D) ---
    // Check if physics should be paused
    let physics_paused = self
      .game
      .as_ref()
      .map(|g| g.is_physics_paused())
      .unwrap_or(false);

    // Skip physics simulation if game requests pause
    if physics_paused {
      return;
    }

    // Run physics: sync entities, step simulation, sync results back
    self.last_world_generation = PhysicsSystem::update(
      &mut self.physics,
      &mut self.world,
      self.config.physics.gravity,
      self.last_world_generation,
    );
  }

  // =============================================================================
  // Render System - CPU & GPU Drawing
  // =============================================================================

  /// Apply Y-based sorting to entities with YSort component
  ///
  /// This recalculates sort keys for Y-sorted entities every frame, combining
  /// their z_order with Y position to achieve Zelda/RPG-style depth sorting.
  fn apply_y_sorting(&mut self) {
    // Calculate sort keys for all YSort entities
    // Sort key = (z_order * 10000) + sort_y, giving layer priority with Y-sorting within layers
    let mut y_sort_keys: Vec<(Entity, i64)> = Vec::with_capacity(self.y_sort_entities.len());

    for &entity in &self.y_sort_entities {
      // Skip inactive entities
      if self.world.has::<super::ecs::Inactive>(entity) {
        continue;
      }

      let Some(renderable) = self.world.get_component::<Renderable>(entity) else {
        continue;
      };

      let Some(y_sort) = self.world.get_component::<YSort>(entity) else {
        continue;
      };

      // Get Y position from WorldTransform or Transform
      let y = if let Some(wt) = self.world.get_component::<WorldTransform>(entity) {
        wt.y
      } else if let Some(t) = self.world.get_component::<Transform>(entity) {
        t.y
      } else {
        continue;
      };

      // Calculate sort Y based on sprite height and foot offset
      let sprite_height = renderable.sprite.height() as f32;
      let sort_y = y_sort.calculate_sort_y(y, sprite_height);

      // Combine z_order with Y position for final sort key
      // z_order * 10000 ensures layer separation, sort_y provides Y-sorting within layers
      let sort_key = (renderable.z_order as i64 * 10000) + (sort_y as i64);

      y_sort_keys.push((entity, sort_key));
    }

    // Update the renderable_cache with new sort keys for YSort entities
    // First, create a map of entity -> sort_key for quick lookup
    let y_sort_map: std::collections::HashMap<Entity, i64> = y_sort_keys.into_iter().collect();

    // Update sort keys in renderable_cache for YSort entities
    for (entity, z_order) in &mut self.renderable_cache {
      if let Some(&sort_key) = y_sort_map.get(entity) {
        // Use the Y-aware sort key (stored as i32, truncated if necessary)
        *z_order = sort_key as i32;
      }
    }

    // Re-sort the entire cache with updated keys
    self.renderable_cache.sort_by_key(|(_, z)| *z);
  }

  /// Render the current frame
  ///
  /// This is the "Render System" - it draws all entities with Transform + Renderable
  fn render(&mut self, _alpha: f32) {
    profile_scope!("cpu:engine_render");

    // Clear batch collector for this frame (used for GPU sprite batching)
    self.batch_collector.clear();

    // --- Rebuild Render Cache (if dirty) ---
    // Rebuild render cache if dirty (entities added/removed or z_order changed)
    // This must happen before we borrow the frame buffer
    if self.render_cache_dirty {
      self.renderable_cache.clear();
      self.y_sort_entities.clear();

      for entity in self.world.entities_with_two::<Transform, Renderable>() {
        if let Some(renderable) = self.world.get_component::<Renderable>(entity) {
          self.renderable_cache.push((entity, renderable.z_order));

          // Track entities with YSort component for per-frame Y-based sorting
          if self.world.has::<YSort>(entity) {
            self.y_sort_entities.push(entity);
          }
        }
      }
      self.renderable_cache.sort_by_key(|(_, z)| *z);

      self.render_cache_dirty = false;
    }

    // --- Apply Y-Sorting (every frame for YSort entities) ---
    // Y-sorting entities need their sort keys recalculated each frame based on Y position
    // This must happen before we borrow the frame buffer
    if !self.y_sort_entities.is_empty() {
      self.apply_y_sorting();
    }

    // Get frame buffer from renderer
    let (frame, frame_width, frame_height) = match &mut self.renderer {
      Some(renderer) => {
        let w = renderer.game_width();
        let h = renderer.game_height();
        (renderer.frame_mut(), w, h)
      }
      None => return,
    };

    // Fill background with transparent black
    // This allows GPU-rendered content (parallax backgrounds) to show through
    // Areas without CPU sprites will be transparent, showing the GPU layer beneath
    for pixel in frame.chunks_exact_mut(4) {
      pixel[0] = 0x00; // R: 0
      pixel[1] = 0x00; // G: 0
      pixel[2] = 0x00; // B: 0
      pixel[3] = 0x00; // A: 0 (transparent)
    }

    // --- Render Background Layers (Parallax, etc.) ---
    // Call game's render_background before entity rendering
    #[allow(deprecated)]
    if let Some(mut game) = self.game.take() {
      game.render_background(frame, frame_width, frame_height, &self.camera);
      self.game = Some(game);
    }

    // --- Render All Entities (CPU Path) ---
    for &(entity, _) in &self.renderable_cache {
      // Skip inactive entities (used by object pooling)
      if self.world.has::<super::ecs::Inactive>(entity) {
        continue;
      }

      let Some(renderable) = self.world.get_component::<Renderable>(entity) else {
        continue;
      };

      if !renderable.visible {
        continue;
      }

      // Use WorldTransform if available (for hierarchical entities),
      // otherwise fall back to local Transform
      let (
        base_x,
        base_y,
        base_rotation,
        base_scale_x,
        base_scale_y,
        pivot_x,
        pivot_y,
        offset_x,
        offset_y,
      ) = if let Some(wt) = self.world.get_component::<WorldTransform>(entity) {
        (
          wt.x,
          wt.y,
          wt.rotation,
          wt.scale_x,
          wt.scale_y,
          wt.pivot_x,
          wt.pivot_y,
          wt.offset_x,
          wt.offset_y,
        )
      } else if let Some(t) = self.world.get_component::<Transform>(entity) {
        (
          t.x, t.y, t.rotation, t.scale_x, t.scale_y, t.pivot_x, t.pivot_y, t.offset_x, t.offset_y,
        )
      } else {
        continue;
      };

      // Apply render interpolation if enabled
      let (x, y, rotation, scale_x, scale_y) = if renderable.interpolate {
        if let Some(prev) = self.world.get_component::<PreviousTransform>(entity) {
          if prev.skip {
            // Skip interpolation this frame (just spawned or teleported)
            (base_x, base_y, base_rotation, base_scale_x, base_scale_y)
          } else {
            // Interpolate between previous and current transform
            let target = Transform {
              x: base_x,
              y: base_y,
              z: 0.0,
              rotation: base_rotation,
              scale_x: base_scale_x,
              scale_y: base_scale_y,
              pivot_x: 0.0,
              pivot_y: 0.0,
              offset_x: 0.0,
              offset_y: 0.0,
            };
            prev.lerp(&target, _alpha)
          }
        } else {
          // No PreviousTransform yet, use current position
          (base_x, base_y, base_rotation, base_scale_x, base_scale_y)
        }
      } else {
        // Interpolation disabled, use current position
        (base_x, base_y, base_rotation, base_scale_x, base_scale_y)
      };

      // Apply offset to base position (offset is a pixel-level adjustment after anchor)
      let x = x + offset_x;
      let y = y + offset_y;

      // Apply YSort depth scaling if present
      let (scale_x, scale_y) = if let Some(y_sort) = self.world.get_component::<YSort>(entity) {
        let (depth_scale_x, depth_scale_y) = y_sort.calculate_depth_scale(y);
        (scale_x * depth_scale_x, scale_y * depth_scale_y)
      } else {
        (scale_x, scale_y)
      };

      // draw_rotated expects degrees, Transform.rotation is in radians
      let rotation_deg = rotation.to_degrees();

      // Capture alpha and tint for use in draw calls
      let alpha = renderable.alpha;
      let tint = renderable.tint;

      match &renderable.style {
        RenderStyle::Single => {
          // Calculate final scale (transform scale * camera zoom)
          let final_scale_x = scale_x * self.camera.zoom;
          let final_scale_y = scale_y * self.camera.zoom;

          // Calculate sprite dimensions after scaling
          let sprite_w = renderable.sprite.width() as f32 * final_scale_x;
          let sprite_h = renderable.sprite.height() as f32 * final_scale_y;

          // Calculate draw position based on pivot point
          // draw_rotated takes the TOP-LEFT corner and rotates around the CENTER.
          //
          // For top-left pivot (0, 0): position IS the top-left, sprite rotates around its center
          // For other pivots: position is the pivot point, we must offset so pivot stays fixed during rotation
          let (draw_x, draw_y) = if pivot_x.abs() < 0.001 && pivot_y.abs() < 0.001 {
            // Top-left pivot (default): original behavior
            // Position is the top-left corner, rotation happens around sprite center
            (x, y)
          } else {
            // Custom pivot point: calculate where top-left should be so pivot stays at (x, y)
            // Offset from pivot to center in sprite-local coords:
            let pivot_to_center_x = (0.5 - pivot_x) * sprite_w;
            let pivot_to_center_y = (0.5 - pivot_y) * sprite_h;

            // Rotate this offset by the sprite's rotation
            let (rotated_offset_x, rotated_offset_y) = if rotation_deg.abs() > 0.01 {
              let angle_rad = rotation_deg.to_radians();
              let cos_a = angle_rad.cos();
              let sin_a = angle_rad.sin();
              (
                pivot_to_center_x * cos_a - pivot_to_center_y * sin_a,
                pivot_to_center_x * sin_a + pivot_to_center_y * cos_a,
              )
            } else {
              (pivot_to_center_x, pivot_to_center_y)
            };

            // The sprite's center in world coords = pivot position + rotated offset
            let center_x = x + rotated_offset_x;
            let center_y = y + rotated_offset_y;

            // The sprite's top-left corner (what draw_rotated expects)
            (center_x - sprite_w / 2.0, center_y - sprite_h / 2.0)
          };

          // Convert to screen position using camera
          let (screen_x, screen_y) = self.camera.world_to_screen(draw_x, draw_y);

          // Cull off-screen sprites (use draw position, not anchor position)
          if !self.camera.is_visible(draw_x, draw_y, sprite_w, sprite_h) {
            continue;
          }

          // Render drop shadow before main sprite (if present)
          if let Some(shadow) = &renderable.drop_shadow {
            // Calculate shadow position (offset from sprite)
            let shadow_x = draw_x + shadow.offset.x;
            let shadow_y = draw_y + shadow.offset.y;

            // Convert to screen position
            let (shadow_screen_x, shadow_screen_y) =
              self.camera.world_to_screen(shadow_x, shadow_y);

            // Apply shadow scale to sprite scale
            let shadow_scale_x = final_scale_x * shadow.scale.x;
            let shadow_scale_y = final_scale_y * shadow.scale.y;

            // Draw shadow sprite with shadow color as tint
            // Combine shadow alpha with renderable alpha
            let shadow_alpha = (shadow.color.a as f32 / 255.0) * alpha;

            renderable.sprite.with_sprite(|sprite| {
              sprite.draw_rotated_tinted(
                frame,
                frame_width,
                frame_height,
                shadow_screen_x as i32,
                shadow_screen_y as i32,
                shadow_scale_x,
                shadow_scale_y,
                rotation_deg,
                renderable.flip_x,
                renderable.flip_y,
                shadow_alpha,
                Color::new(shadow.color.r, shadow.color.g, shadow.color.b, 255), // Full alpha in tint, alpha handled separately
              );
            });
          }

          // Render blob shadow before main sprite (if present)
          if let Some(blob) = &renderable.blob_shadow {
            // Get effective radius and alpha (accounts for height scaling)
            let (eff_radius_x, eff_radius_y) = blob.effective_radius();
            let blob_alpha = blob.effective_alpha() * alpha;

            // Skip if shadow is too small or transparent
            if eff_radius_x >= 1.0 && eff_radius_y >= 1.0 && blob_alpha > 0.01 {
              // Calculate blob shadow position (centered below sprite, with offset)
              // Use the sprite's base position (not the rotated draw position)
              let blob_x = x - eff_radius_x;
              let blob_y = y + blob.offset_y - eff_radius_y;

              let (blob_screen_x, blob_screen_y) = self.camera.world_to_screen(blob_x, blob_y);

              // Create ellipse sprite for the blob shadow
              let blob_w = (eff_radius_x * 2.0) as u32;
              let blob_h = (eff_radius_y * 2.0) as u32;
              let blob_sprite = Sprite::solid_ellipse(
                blob_w,
                blob_h,
                blob.color.r,
                blob.color.g,
                blob.color.b,
                255,
              );

              // Draw with camera zoom
              blob_sprite.draw_rotated_tinted(
                frame,
                frame_width,
                frame_height,
                blob_screen_x as i32,
                blob_screen_y as i32,
                self.camera.zoom,
                self.camera.zoom,
                0.0, // No rotation for blob shadows
                false,
                false,
                blob_alpha,
                Color::WHITE,
              );
            }
          }

          // Render contact shadow before main sprite (if present)
          // Contact shadows are small concentrated shadows at the contact point
          if let Some(contact) = &renderable.contact_shadow {
            let contact_alpha = contact.intensity * alpha;

            // Skip if shadow is too small or transparent
            if contact.radius >= 1.0 && contact_alpha > 0.01 {
              // Contact shadow is positioned at the base of the sprite
              // Using a squashed ellipse (width = radius, height = radius/2)
              let contact_radius_x = contact.radius;
              let contact_radius_y = contact.radius * 0.4; // More squashed than blob shadows

              let contact_x = x - contact_radius_x;
              let contact_y = y + contact.offset_y - contact_radius_y;

              let (contact_screen_x, contact_screen_y) =
                self.camera.world_to_screen(contact_x, contact_y);

              // Create ellipse sprite for the contact shadow
              let contact_w = (contact_radius_x * 2.0) as u32;
              let contact_h = (contact_radius_y * 2.0) as u32;
              let contact_sprite = Sprite::solid_ellipse(
                contact_w.max(2),
                contact_h.max(2),
                contact.color.r,
                contact.color.g,
                contact.color.b,
                255,
              );

              // Draw with camera zoom
              contact_sprite.draw_rotated_tinted(
                frame,
                frame_width,
                frame_height,
                contact_screen_x as i32,
                contact_screen_y as i32,
                self.camera.zoom,
                self.camera.zoom,
                0.0, // No rotation for contact shadows
                false,
                false,
                contact_alpha,
                Color::WHITE,
              );
            }
          }

          // Check if sprite has a shader effect to apply
          if let Some(SpriteShader::BuiltIn(effect)) = &renderable.shader {
            // Apply CPU-based shader effect
            renderable.sprite.with_sprite(|sprite| {
              let src_width = sprite.width();
              let src_height = sprite.height();
              let (transformed_data, new_width, new_height) =
                effect.apply_cpu(sprite.data(), src_width, src_height);

              // For expanded sprites (outline/glow), we need to adjust position to keep sprite centered
              // Use round() instead of truncation to avoid asymmetric clipping with non-uniform scales
              let offset_x = ((new_width as i32 - src_width as i32) / 2) as f32 * final_scale_x;
              let offset_y = ((new_height as i32 - src_height as i32) / 2) as f32 * final_scale_y;

              let transformed_sprite = Sprite::from_rgba(transformed_data, new_width, new_height);
              transformed_sprite.draw_rotated_tinted(
                frame,
                frame_width,
                frame_height,
                screen_x as i32 - offset_x.round() as i32,
                screen_y as i32 - offset_y.round() as i32,
                final_scale_x,
                final_scale_y,
                rotation_deg,
                renderable.flip_x,
                renderable.flip_y,
                alpha,
                tint,
              );
            });
          } else {
            // No shader - draw normally with alpha/tint
            renderable.sprite.with_sprite(|sprite| {
              sprite.draw_rotated_tinted(
                frame,
                frame_width,
                frame_height,
                screen_x as i32,
                screen_y as i32,
                final_scale_x,
                final_scale_y,
                rotation_deg,
                renderable.flip_x,
                renderable.flip_y,
                alpha,
                tint,
              );
            });
          }
        }

        RenderStyle::Tiled {
          direction,
          target_size,
        } => {
          let tile_size = if direction.is_horizontal() {
            renderable.sprite.width() as f32
          } else {
            renderable.sprite.height() as f32
          };

          if tile_size <= 0.0 {
            continue;
          }

          let tile_count = (target_size / tile_size).ceil() as u32;
          let (offset_dx, offset_dy) = direction.offset(tile_size);

          // Common values for both shadow and main passes
          let rotation_rad = rotation_deg.to_radians();
          let cos_r = rotation_rad.cos();
          let sin_r = rotation_rad.sin();
          let final_scale_x = scale_x * self.camera.zoom;
          let final_scale_y = scale_y * self.camera.zoom;

          // Render drop shadows for all tiles first (if present)
          if let Some(shadow) = &renderable.drop_shadow {
            let shadow_alpha = (shadow.color.a as f32 / 255.0) * alpha;
            let shadow_scale_x = final_scale_x * shadow.scale.x;
            let shadow_scale_y = final_scale_y * shadow.scale.y;
            let shadow_tint = Color::new(shadow.color.r, shadow.color.g, shadow.color.b, 255);

            for i in 0..tile_count {
              let tile_offset_x = offset_dx * i as f32;
              let tile_offset_y = offset_dy * i as f32;

              let (start_x, start_y) = match direction {
                TileDirection::Up => (0.0, -(tile_count as f32 - 1.0) * tile_size),
                TileDirection::Left => (-(tile_count as f32 - 1.0) * tile_size, 0.0),
                TileDirection::Down | TileDirection::Right => (0.0, 0.0),
              };

              let local_x = start_x + tile_offset_x;
              let local_y = start_y + tile_offset_y;
              let rotated_x = local_x * cos_r - local_y * sin_r;
              let rotated_y = local_x * sin_r + local_y * cos_r;
              let scaled_x = rotated_x * scale_x;
              let scaled_y = rotated_y * scale_y;

              // Shadow position = tile position + shadow offset
              let world_x = x + scaled_x + shadow.offset.x;
              let world_y = y + scaled_y + shadow.offset.y;

              let (screen_x, screen_y) = self.camera.world_to_screen(world_x, world_y);

              renderable.sprite.with_sprite(|sprite| {
                sprite.draw_rotated_tinted(
                  frame,
                  frame_width,
                  frame_height,
                  screen_x as i32,
                  screen_y as i32,
                  shadow_scale_x,
                  shadow_scale_y,
                  rotation_deg,
                  renderable.flip_x,
                  renderable.flip_y,
                  shadow_alpha,
                  shadow_tint,
                );
              });
            }
          }

          // Render main tiles
          for i in 0..tile_count {
            let tile_offset_x = offset_dx * i as f32;
            let tile_offset_y = offset_dy * i as f32;

            // For Up/Left directions, start from negative offset
            let (start_x, start_y) = match direction {
              TileDirection::Up => (0.0, -(tile_count as f32 - 1.0) * tile_size),
              TileDirection::Left => (-(tile_count as f32 - 1.0) * tile_size, 0.0),
              TileDirection::Down | TileDirection::Right => (0.0, 0.0),
            };

            // Calculate tile position relative to entity, then rotate
            let local_x = start_x + tile_offset_x;
            let local_y = start_y + tile_offset_y;
            let rotated_x = local_x * cos_r - local_y * sin_r;
            let rotated_y = local_x * sin_r + local_y * cos_r;

            // Apply scale to rotated offset
            let scaled_x = rotated_x * scale_x;
            let scaled_y = rotated_y * scale_y;

            let world_x = x + scaled_x;
            let world_y = y + scaled_y;

            let (screen_x, screen_y) = self.camera.world_to_screen(world_x, world_y);

            renderable.sprite.with_sprite(|sprite| {
              sprite.draw_rotated_tinted(
                frame,
                frame_width,
                frame_height,
                screen_x as i32,
                screen_y as i32,
                final_scale_x,
                final_scale_y,
                rotation_deg,
                renderable.flip_x,
                renderable.flip_y,
                alpha,
                tint,
              );
            });
          }
        }
      }
    }

    // === Render CompositeTile components ===
    // These are multi-part tiles with static and tiled sections
    for entity in self.world.entities_with_two::<Transform, CompositeTile>() {
      let Some(transform) = self.world.get_component::<Transform>(entity) else {
        continue;
      };
      let Some(composite) = self.world.get_component::<CompositeTile>(entity) else {
        continue;
      };

      if !composite.visible {
        continue;
      }

      let final_scale_x = transform.scale_x * self.camera.zoom;
      let final_scale_y = transform.scale_y * self.camera.zoom;

      for part in &composite.parts {
        let flip_x = part.flip_x;
        let flip_y = part.flip_y;

        match &part.tiling {
          None => {
            // Static part - draw once
            let world_x = transform.x + part.offset_x;
            let world_y = transform.y + part.offset_y;
            let (screen_x, screen_y) = self.camera.world_to_screen(world_x, world_y);

            part.sprite.with_sprite(|sprite| {
              sprite.draw_scaled_xy_flipped(
                frame,
                frame_width,
                frame_height,
                screen_x as i32,
                screen_y as i32,
                final_scale_x,
                final_scale_y,
                flip_x,
                flip_y,
              );
            });
          }
          Some((direction, target_size)) => {
            // Tiled part - draw multiple times
            let tile_size = if direction.is_horizontal() {
              part.sprite.width() as f32
            } else {
              part.sprite.height() as f32
            };

            if tile_size <= 0.0 || *target_size <= 0.0 {
              continue;
            }

            let tile_count = (target_size / tile_size).ceil() as u32;
            let (offset_dx, offset_dy) = direction.offset(tile_size);

            for i in 0..tile_count {
              let tile_offset_x = offset_dx * i as f32;
              let tile_offset_y = offset_dy * i as f32;

              // For Up/Left directions, adjust starting position
              let (start_x, start_y) = match direction {
                TileDirection::Up => (0.0, -(tile_count as f32 - 1.0) * tile_size),
                TileDirection::Left => (-(tile_count as f32 - 1.0) * tile_size, 0.0),
                TileDirection::Down | TileDirection::Right => (0.0, 0.0),
              };

              let world_x = transform.x + part.offset_x + start_x + tile_offset_x;
              let world_y = transform.y + part.offset_y + start_y + tile_offset_y;
              let (screen_x, screen_y) = self.camera.world_to_screen(world_x, world_y);

              part.sprite.with_sprite(|sprite| {
                sprite.draw_scaled_xy_flipped(
                  frame,
                  frame_width,
                  frame_height,
                  screen_x as i32,
                  screen_y as i32,
                  final_scale_x,
                  final_scale_y,
                  flip_x,
                  flip_y,
                );
              });
            }
          }
        }
      }
    }

    // --- Debug Overlays ---
    if self.config.debug.show_colliders {
      debug::render_colliders(frame, frame_width, frame_height, &self.world, &self.camera);
    }
    if self.config.debug.show_velocities {
      debug::render_velocities(frame, frame_width, frame_height, &self.world, &self.camera);
    }
    if self.config.debug.show_origins {
      debug::render_origins(frame, frame_width, frame_height, &self.world, &self.camera);
    }

    // --- UI & FPS Counter ---
    // Call game's render_ui with UiContext for immediate-mode UI
    // UI is always rendered to a separate buffer and composited last, ensuring it appears
    // on top of trails, GPU sprites, lighting, and post-processing effects.
    self.ui_buffer.fill(0);
    if let Some(mut game) = self.game.take() {
      let click_consumed;
      let escape_consumed;
      {
        let mut ui_ctx = UiContext::new_scaled(
          &mut self.ui_buffer,
          frame_width,
          frame_height,
          &self.input,
          &self.ui_font,
          self.ui_scale,
        );

        // When the console is visible and the mouse is over it, block all mouse
        // interaction (hover, click, hold) so game UI underneath can't respond.
        if self.console_state.visible {
          let (mx, my) = self.input.mouse_position();
          if self.console_state.window_rect().contains(mx, my) {
            ui_ctx.set_mouse_occluded();
            ui_ctx.consume_click();
          }
        }

        game.render_ui(&mut ui_ctx);

        // Render debug overlays on top of game UI (always visible)
        // Render order: stats (top) -> FPS -> frame timing (bottom)
        if self.config.debug.show_stats {
          debug::render_stats(&mut ui_ctx, &self.smoothed_stats, frame_height);
        }
        if self.config.debug.show_fps {
          debug::render_fps_counter(
            &mut ui_ctx,
            self.smoothed_timings.fps as u64,
            frame_height,
            self.config.debug.show_frame_timing,
          );
        }
        if self.config.debug.show_frame_timing {
          debug::render_frame_timing(&mut ui_ctx, &self.smoothed_timings, frame_height);
        }

        // Render developer console on top of everything
        self
          .console_state
          .render(&mut ui_ctx, frame_width, frame_height);

        click_consumed = ui_ctx.click_consumed();
        escape_consumed = ui_ctx.escape_consumed();
      } // ui_ctx drops here, releasing &self.input borrow

      // Propagate UI consumption to InputState so consumed events
      // don't leak to the next frame's update
      if click_consumed {
        self.input.consume_mouse_press(MouseButton::Left);
      }
      if escape_consumed {
        self.input.consume_key_press(KeyCode::Escape);
      }

      self.game = Some(game);
    }

    // --- Screen Overlay (for transitions) ---
    if let Some(game) = &self.game
      && let Some((alpha, color)) = game.get_overlay()
      && alpha > 0.001
    {
      blend_overlay(frame, color, alpha);
    }

    // Note: UI compositing for non-lit UI is now done on GPU in render_with_lighting
    // The ui_buffer is passed to the renderer and composited after the lighting pass
  }
}

/// Blend a color overlay over the entire frame buffer.
///
/// This is used for transition effects (fade to black, etc.).
/// Alpha of 0.0 = no change, 1.0 = fully replaced with overlay color.
#[inline]
fn blend_overlay(frame: &mut [u8], color: [u8; 4], alpha: f32) {
  let inv_alpha = 1.0 - alpha;
  let overlay_r = color[0] as f32 * alpha;
  let overlay_g = color[1] as f32 * alpha;
  let overlay_b = color[2] as f32 * alpha;

  for pixel in frame.chunks_exact_mut(4) {
    pixel[0] = (pixel[0] as f32 * inv_alpha + overlay_r) as u8;
    pixel[1] = (pixel[1] as f32 * inv_alpha + overlay_g) as u8;
    pixel[2] = (pixel[2] as f32 * inv_alpha + overlay_b) as u8;
    // pixel[3] (alpha) stays unchanged
  }
}

// =============================================================================
// Window Event Handler
// =============================================================================

impl ApplicationHandler for Engine {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let resizable = self.config.window_mode != WindowMode::Fixed;

    // For borderless/fullscreen, we still create with the resolution but apply fullscreen after
    // Window starts hidden to prevent white flash - shown after first frame is rendered
    let window_attrs = WindowAttributes::default()
      .with_title(&self.config.title)
      .with_inner_size(winit::dpi::LogicalSize::new(
        self.config.width,
        self.config.height,
      ))
      .with_resizable(resizable)
      .with_decorations(self.config.display_mode != DisplayMode::BorderlessFullscreen)
      .with_visible(false);

    let window = Arc::new(
      event_loop
        .create_window(window_attrs)
        .expect("Failed to create window"),
    );

    // Apply display mode (fullscreen/borderless)
    match self.config.display_mode {
      DisplayMode::Windowed => {
        // Already windowed, nothing to do
      }
      DisplayMode::ExclusiveFullscreen => {
        // Exclusive fullscreen - use current monitor's native resolution
        if let Some(monitor) = window.current_monitor()
          && let Some(video_mode) = monitor.video_modes().next()
        {
          window.set_fullscreen(Some(winit::window::Fullscreen::Exclusive(video_mode)));
        }
      }
      DisplayMode::BorderlessFullscreen => {
        // Borderless fullscreen - fill screen without mode change
        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(
          window.current_monitor(),
        )));
      }
    }

    // Enable IME for text input support
    window.set_ime_allowed(true);

    // Set window icon (custom or default Ryo2D logo)
    if let Some(icon) = Self::load_window_icon(self.config.icon_path.as_deref()) {
      window.set_window_icon(Some(icon));
    }

    // Create wgpu renderer (blocking on async)
    let mut renderer = pollster::block_on(WgpuRenderer::new(
      Arc::clone(&window),
      self.config.width,
      self.config.height,
      self.config.window_mode,
      self.config.max_width,
      self.config.max_height,
      self.vsync,
    ));

    // Create GPU sprite renderer using the game texture format (not surface format)
    let sprite_gpu = SpriteGpuRenderer::new(
      renderer.device(),
      renderer.queue(),
      renderer.game_texture_format(),
    );

    // Create 2D lighting renderer (disabled by default, enabled when Light2D entities exist)
    let lighting = LightingRenderer::new(
      renderer.device(),
      self.config.width,
      self.config.height,
      renderer.game_texture_format(),
    );

    // Create trail/line renderer for motion trails and procedural lines
    let trail_renderer = TrailRenderer::new(
      renderer.device(),
      self.config.width,
      self.config.height,
      renderer.game_texture_format(),
    );

    // Present the first frame immediately to prevent white flash on startup
    // The renderer's pixel buffer is pre-filled with warm cream background
    let _ = renderer.render();

    // Now that the first frame is rendered, show the window
    window.set_visible(true);

    // Store window's DPI scale factor
    self.scale_factor = window.scale_factor();

    // Compute UI scale from config or auto-detect from DPI
    self.ui_scale = self.config.ui_scale.unwrap_or_else(|| {
      // Auto-detect: round scale_factor to nearest integer, minimum 1
      (self.scale_factor.round() as u32).max(1)
    });

    self.window = Some(window);
    self.renderer = Some(renderer);
    self.sprite_gpu = Some(sprite_gpu);
    self.lighting = Some(lighting);
    self.trail_renderer = Some(trail_renderer);
    self.time = Some(TimeState::new(
      self.config.target_fps,
      self.config.physics_hz,
    ));

    // Initialize profiler (no-op if `profiler` feature is disabled or enable_profiler=false)
    // This enables puffin scope collection so F10 can open the profiler window later
    super::profiler::init(self.config.debug.enable_profiler);

    // Auto-open profiler window only when explicitly requested via CLI (-p flag)
    // When enable_profiler=true in Ryo.toml, the profiler is available via F10 but doesn't auto-open
    #[cfg(feature = "profiler")]
    if std::env::var("RYO_PROFILER_AUTO_OPEN").is_ok()
      && let Some(ref renderer) = self.renderer
    {
      self.profiler_window = super::windows::profiler_window::ProfilerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        renderer.instance(),
        renderer.adapter(),
      );
    }

    // Create logger window if enabled
    if self.config.debug.logger_window
      && let Some(ref renderer) = self.renderer
    {
      self.logger_window = super::windows::logger_window::LoggerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        renderer.instance(),
        renderer.adapter(),
      );
    }

    // Call game init if not already initialized
    if !self.initialized {
      if let Some(mut game) = self.game.take() {
        // TimeInfo for init - no timing data yet, use defaults
        let time_info = TimeInfo {
          fixed_dt: self
            .time
            .as_ref()
            .map(|t| t.fixed_dt())
            .unwrap_or(1.0 / 60.0),
          scaled_dt: 0.0,
          real_dt: 0.0,
          render_alpha: 0.0,
          time_scale: 1.0,
          is_hitstop: false,
          frame_count: 0,
        };

        let mut ctx = GameContext {
          world: &mut self.world,
          camera: &mut self.camera,
          input: &mut self.input,
          assets: &mut self.assets,
          audio: &mut self.audio,
          effects: &mut self.effects,
          lighting: &mut self.lighting,
          events: &mut self.events,
          window_request: &mut self.window_request,
          cursor_request: &mut self.cursor_request,
          time_control: &mut self.time_control,
          time: time_info,
          scale_factor: self.scale_factor,
          command_registry: &mut self.command_registry,
          scope_id: 0, // Game-level scope (permanent)
        };
        game.init(&mut ctx);
        self.game = Some(game);
      }
      self.initialized = true;
      // Entities were spawned during init, rebuild render cache
      self.render_cache_dirty = true;
    }

    engine_info!(module: "Init", "Engine initialized successfully!");
    engine_info!(
      module: "Init",
      "Game loop using fixed timestep: {}Hz updates",
      self.config.physics_hz
    );
  }

  #[allow(unused_variables)] // window_id used only when profiler feature is enabled
  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    window_id: WindowId,
    event: WindowEvent,
  ) {
    // Check if this event is for the profiler window
    #[cfg(feature = "profiler")]
    if let Some(ref mut profiler) = self.profiler_window
      && window_id == profiler.window_id()
    {
      // Handle profiler window events
      match &event {
        WindowEvent::CloseRequested => {
          // Close profiler window but don't exit the app
          self.profiler_window = None;
          return;
        }
        WindowEvent::RedrawRequested => {
          profiler.render();
          return;
        }
        WindowEvent::Focused(focused) => {
          // Track profiler window focus separately
          self.profiler_window_focused = *focused;
          return;
        }
        _ => {
          profiler.handle_event(&event);
          return;
        }
      }
    }

    // Check if this event is for the logger window
    if let Some(ref mut logger) = self.logger_window
      && window_id == logger.window_id()
    {
      // Handle logger window events
      match &event {
        WindowEvent::CloseRequested => {
          // Close logger window but don't exit the app
          self.logger_window = None;
          return;
        }
        WindowEvent::RedrawRequested => {
          logger.render();
          return;
        }
        WindowEvent::Focused(focused) => {
          // Track logger window focus separately
          self.logger_window_focused = *focused;
          return;
        }
        _ => {
          logger.handle_event(&event);
          return;
        }
      }
    }

    // Handle main window input events
    match &event {
      WindowEvent::KeyboardInput { event, .. } => {
        if let PhysicalKey::Code(key) = event.physical_key {
          self.input.handle_keyboard(key, event.state);
        }
        // Capture text input from key events (winit 0.30+)
        // This handles regular character input when IME is not composing
        if event.state == winit::event::ElementState::Pressed
          && let Some(text) = &event.text
        {
          // Don't capture control characters (backspace, enter, etc.)
          // These are handled separately as key presses
          // Also skip when Ctrl or Alt is held (for shortcuts like Ctrl+A, Ctrl+C)
          let ctrl_held =
            self.input.key_down(KeyCode::ControlLeft) || self.input.key_down(KeyCode::ControlRight);
          let alt_held =
            self.input.key_down(KeyCode::AltLeft) || self.input.key_down(KeyCode::AltRight);
          if !ctrl_held && !alt_held && !text.chars().all(|c| c.is_control()) {
            self.input.handle_text_input(text);
          }
        }
      }
      WindowEvent::MouseInput { state, button, .. } => {
        self.input.handle_mouse_button(*button, *state);
      }
      WindowEvent::CursorMoved { position, .. } => {
        // Transform window coordinates to game coordinates
        // If None, cursor is in black bars (letterbox area) - don't update mouse position
        if let Some((game_x, game_y)) = self.window_to_game_coords(position.x, position.y) {
          self.input.handle_mouse_move(game_x as f64, game_y as f64);
        }
      }
      WindowEvent::MouseWheel { delta, .. } => {
        self.input.handle_scroll(*delta);
      }
      WindowEvent::Ime(ime_event) => {
        // Handle Input Method Editor (IME) events for text input
        match ime_event {
          Ime::Enabled => {
            self.input.handle_ime_enabled();
          }
          Ime::Disabled => {
            self.input.handle_ime_disabled();
          }
          Ime::Preedit(text, cursor) => {
            self.input.handle_ime_preedit(text.clone(), *cursor);
          }
          Ime::Commit(text) => {
            self.input.handle_ime_commit(text.clone());
          }
        }
      }
      WindowEvent::Resized(new_size) => {
        // Handle window resize based on window mode
        if let Some(renderer) = &mut self.renderer {
          // For stretch mode, resize the internal buffer to match window
          if self.config.window_mode == WindowMode::ResizableStretch {
            renderer.resize_buffer(new_size.width, new_size.height);
            // Resize lighting renderer to match new game buffer size
            if let Some(lighting) = &mut self.lighting {
              lighting.resize(renderer.device(), new_size.width, new_size.height);
            }
            // Update camera to match new buffer size
            self.camera = Camera::centered_at(
              new_size.width as f32 / 2.0,
              new_size.height as f32 / 2.0,
              new_size.width,
              new_size.height,
            );
            // Mark render cache as dirty since viewport changed
            self.render_cache_dirty = true;
          }

          // For all modes, update surface size for proper scaling
          renderer.resize(new_size.width, new_size.height);
          self.last_window_size = (new_size.width, new_size.height);
        }

        // Request immediate redraw to update display
        if let Some(window) = &self.window {
          window.request_redraw();
        }
      }
      WindowEvent::Focused(focused) => {
        self.game_window_focused = *focused;
      }
      WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
        // Track DPI scale factor changes (e.g., moving window between monitors)
        self.scale_factor = *scale_factor;

        // Update UI scale if auto-detect is enabled
        if self.config.ui_scale.is_none() {
          self.ui_scale = (self.scale_factor.round() as u32).max(1);
        }

        // Resize event will follow automatically, no additional action needed
      }
      _ => {}
    }

    match event {
      WindowEvent::CloseRequested => {
        engine_info!(module: "Engine", "Close requested, exiting...");
        event_loop.exit();
      }
      WindowEvent::RedrawRequested => {
        // Signal profiler that a new frame is starting (no-op if feature disabled)
        // Also track wall-clock frame time for profiler display
        #[cfg(feature = "profiler")]
        if let Some(ref mut profiler) = self.profiler_window {
          profiler.track_frame_time();
        }
        super::profiler::new_frame();

        // Handle surface reconfiguration if needed (after display mode change)
        if self.surface_needs_reconfigure {
          self.surface_needs_reconfigure = false;
          if let (Some(window), Some(renderer)) = (&self.window, &mut self.renderer) {
            let size = window.inner_size();
            if size.width > 0 && size.height > 0 {
              renderer.resize(size.width, size.height);
            }
          }
        }

        // Reset frame timings
        self.frame_timings = FrameTimings::default();
        let frame_start = std::time::Instant::now();

        if let Some(time) = &mut self.time {
          time.begin_frame();
        }

        // Handle debug key toggles (F1-F4)
        // These are consumed immediately to prevent double-firing if no update runs this frame
        if self.debug_keys_enabled {
          // F1: Cycle overlay mode
          if self.input.key_pressed(KeyCode::F1) {
            self.overlay_mode = self.overlay_mode.next();
            let (show_fps, show_frame_timing, show_stats) = self.overlay_mode.to_flags();
            self.config.debug.show_fps = show_fps;
            self.config.debug.show_frame_timing = show_frame_timing;
            self.config.debug.show_stats = show_stats;
            self.input.consume_key(KeyCode::F1);
          }
          // F2: Toggle collision boundaries
          if self.input.key_pressed(KeyCode::F2) {
            self.config.debug.show_colliders = !self.config.debug.show_colliders;
            self.input.consume_key(KeyCode::F2);
          }
          // F3: Toggle velocity vectors
          if self.input.key_pressed(KeyCode::F3) {
            self.config.debug.show_velocities = !self.config.debug.show_velocities;
            self.input.consume_key(KeyCode::F3);
          }
          // F4: Toggle origin points
          if self.input.key_pressed(KeyCode::F4) {
            self.config.debug.show_origins = !self.config.debug.show_origins;
            self.input.consume_key(KeyCode::F4);
          }
          // F5: Toggle borderless window
          if self.input.key_pressed(KeyCode::F5) {
            self.pending_toggle_borderless = true;
            self.input.consume_key(KeyCode::F5);
          }
        }

        // Dev console toggle (backtick/grave key)
        if self.config.debug.dev_console && self.input.key_pressed(KeyCode::Backquote) {
          self.console_state.toggle();
          self.input.consume_key(KeyCode::Backquote);
        }

        // Handle dev window keys (F9-F10)
        // These are separate from debug_keys_enabled and controlled by dev_window_keys
        if self.config.debug.dev_window_keys {
          // F9: Toggle logger window
          if self.input.key_pressed(KeyCode::F9) {
            if self.logger_window.is_some() {
              // Close the window
              self.logger_window = None;
              self.logger_window_focused = false;
            } else {
              // Request to open (will be created in event loop where we have ActiveEventLoop)
              self.pending_open_logger = true;
            }
            self.input.consume_key(KeyCode::F9);
          }

          // F10: Toggle profiler window
          #[cfg(feature = "profiler")]
          if self.input.key_pressed(KeyCode::F10) {
            if self.profiler_window.is_some() {
              // Close the window
              self.profiler_window = None;
              self.profiler_window_focused = false;
            } else {
              // Request to open (will be created in event loop where we have ActiveEventLoop)
              self.pending_open_profiler = true;
            }
            self.input.consume_key(KeyCode::F10);
          }
        }

        // Process gamepad events
        if let Some(ref mut gilrs) = self.gilrs {
          // Check for newly connected gamepads
          for (id, gamepad) in gilrs.gamepads() {
            if gamepad.is_connected() {
              self
                .input
                .try_set_active_gamepad(id, gamepad.name().to_string());
            }
          }

          // Process all pending events
          while let Some(gilrs::Event { id, event, .. }) = gilrs.next_event() {
            match event {
              gilrs::EventType::Connected => {
                if let Some(gamepad) = gilrs.connected_gamepad(id) {
                  self
                    .input
                    .handle_gamepad_connected(id, gamepad.name().to_string());
                }
              }
              gilrs::EventType::Disconnected => {
                self.input.handle_gamepad_disconnected(id);
              }
              gilrs::EventType::ButtonPressed(button, _) => {
                self.input.handle_gamepad_button(id, button, true);
              }
              gilrs::EventType::ButtonReleased(button, _) => {
                self.input.handle_gamepad_button(id, button, false);
              }
              gilrs::EventType::ButtonChanged(button, value, _) => {
                // For analog buttons like triggers
                self.input.handle_gamepad_trigger(id, button, value);
              }
              gilrs::EventType::AxisChanged(axis, value, _) => {
                self.input.handle_gamepad_axis(id, axis, value);
              }
              _ => {}
            }
          }
        }

        // Get real dt for systems that should use unscaled time (shake, hitstop decay, UI)
        let real_dt = self.time.as_ref().map(|t| t.fixed_dt()).unwrap_or(0.0);

        // Update camera shake with real (unscaled) dt
        self.camera.update_shake(real_dt);

        // Update developer console (cursor blink, etc.)
        self.console_state.update(real_dt);

        // Handle console input when visible
        if self.console_state.visible {
          // Console handles its own input and returns command if submitted
          if let Some(command_str) = self.console_state.handle_input(&mut self.input) {
            // Execute the command
            let commands = self.command_registry.command_info_list();
            let mut cmd_ctx = CommandContext {
              console: &mut self.console_state,
              debug_config: &mut self.config.debug,
              quit_requested: false,
              time_scale: &mut self.time_control.time_scale,
              commands,
              game_name: self.game_name.clone(),
              game_version: self.game_version.clone(),
            };

            let result = self.command_registry.execute(&command_str, &mut cmd_ctx);

            // Handle quit request from console
            if matches!(result, CommandResult::Quit) || cmd_ctx.quit_requested {
              self.console_quit_requested = true;
            }
          }

          // Only consume keyboard input when console is FOCUSED
          // This allows playing the game with the console open but unfocused
          if self.console_state.is_focused() {
            self.input.clear_keyboard_state();
          }
        }

        // Update cursor based on console state (resize handle hover, etc.)
        // This runs even when console is closed to reset the cursor properly
        let (mouse_x, mouse_y) = self.input.mouse_position();
        if let Some(cursor_style) = self.console_state.desired_cursor(mouse_x, mouse_y) {
          self.cursor_request.cursor = Some(cursor_style);
        }

        // Fixed timestep update loop (includes game logic + physics)
        // Use a block to limit the profile_scope lifetime
        let mut did_update = false;
        {
          profile_scope!("cpu:update");
          let update_start = std::time::Instant::now();

          // Snapshot transforms for interpolation BEFORE any updates
          // This captures the "previous" state that we'll lerp from during rendering
          InterpolationSystem::snapshot(&mut self.world);

          let mut first_update = true;
          while self
            .time
            .as_ref()
            .map(|t| t.should_update())
            .unwrap_or(false)
          {
            // Sync time_control to TimeState before each update
            if let Some(time) = &mut self.time {
              time.sync_from_time_control(&self.time_control);
            }

            // Use scaled dt for game logic (affected by time_scale and hitstop)
            let scaled_dt = self
              .time
              .as_ref()
              .expect("TimeState should be initialized")
              .scaled_fixed_dt();
            self.update(scaled_dt);

            if first_update {
              self.input.suppress_oneshots();
              first_update = false;
            }

            if let Some(time) = &mut self.time {
              time.consume_update();
            }
            did_update = true;
          }
          self.input.unsuppress_oneshots();

          // After all updates: decay hitstop and sync back to time_control
          if let Some(time) = &mut self.time {
            time.sync_from_time_control(&self.time_control);
            time.update_hitstop(real_dt);
            time.sync_to_time_control(&mut self.time_control);
          }
          // Update time includes both game logic and physics
          self.frame_timings.update = update_start.elapsed();
        } // update scope ends here

        // Check if game or console wants to quit
        if self.game.as_ref().map(|g| g.should_quit()).unwrap_or(false)
          || self.console_quit_requested
        {
          event_loop.exit();
          return;
        }

        // Render with interpolation alpha
        // Use a block to limit the profile_scope lifetime
        {
          profile_scope!("cpu:render");
          let render_start = std::time::Instant::now();
          let alpha = self
            .time
            .as_ref()
            .map(|t| t.interpolation_alpha())
            .unwrap_or(0.0);
          self.render(alpha);

          // Call game.render_gpu() to populate GPU sprite batches
          if let Some(mut game) = self.game.take() {
            // Get device/queue from renderer for texture uploads
            if let (Some(sprite_gpu), Some(renderer)) =
              (self.sprite_gpu.as_mut(), self.renderer.as_ref())
            {
              // Upload debug font texture on first frame (if debug overlays enabled)
              if !self.debug_font_uploaded
                && (self.config.debug.show_fps
                  || self.config.debug.show_frame_timing
                  || self.config.debug.show_stats)
              {
                debug::upload_debug_font(
                  &self.ui_font,
                  sprite_gpu,
                  renderer.device(),
                  renderer.queue(),
                );
                self.debug_font_uploaded = true;
              }

              let mut gpu_ctx = GpuRenderContext::new(
                &mut self.batch_collector,
                sprite_gpu,
                renderer.device(),
                renderer.queue(),
                &self.camera,
                &self.world,
              );
              game.render_gpu(&mut gpu_ctx);
            }
            self.game = Some(game);
          }

          // Capture debug stats before rendering clears them
          // Count CPU sprites (visible, active entities with Renderable)
          let cpu_sprite_count = self
            .renderable_cache
            .iter()
            .filter(|(entity, _)| {
              !self.world.has::<super::ecs::Inactive>(*entity)
                && self
                  .world
                  .get_component::<Renderable>(*entity)
                  .map(|r| r.visible)
                  .unwrap_or(false)
            })
            .count();

          self.debug_stats = DebugStats {
            entity_count: self.world.len(),
            draw_calls: self.batch_collector.batch_count() as u32,
            sprite_count: (cpu_sprite_count + self.batch_collector.sprite_count()) as u32,
          };
          self.smoothed_stats.update(&self.debug_stats);

          // Render time includes software rendering + GPU batch collection
          self.frame_timings.render = render_start.elapsed();
        } // render scope ends here

        // Note: Debug overlays (FPS, frame timing) are now rendered via UI context
        // in the UI rendering section below, so they appear on top of everything
        // including lighting effects.

        // Actually draw to screen with post-processing effects, lighting, and GPU sprites
        // Take sprite_gpu, lighting, and trail_renderer out to avoid borrow conflicts with renderer
        profile_scope!("gpu:present");
        let present_start = std::time::Instant::now();
        let mut sprite_gpu = self.sprite_gpu.take();
        let mut renderer = self.renderer.take();
        let mut lighting = self.lighting.take();
        let mut trail_renderer = self.trail_renderer.take();

        if let (Some(rend), Some(gpu)) = (&mut renderer, &mut sprite_gpu) {
          // Collect trails and lines from the world (inside renderer block for device/queue access)
          if let Some(ref mut trails) = trail_renderer {
            trails.begin_frame();

            // Add Trail2D components (skip inactive pooled entities)
            for entity in self.world.entities_with::<Trail2D>() {
              if self.world.has::<super::ecs::Inactive>(entity) {
                continue;
              }
              if let (Some(trail), Some(transform)) = (
                self.world.get_component::<Trail2D>(entity),
                self.world.get_component::<Transform>(entity),
              ) {
                // Use add_trail_with_device for texture caching support
                trails.add_trail_with_device(rend.device(), rend.queue(), &trail, &transform);
              }
            }

            // Add Line2D components (skip inactive pooled entities)
            for entity in self.world.entities_with::<Line2D>() {
              if self.world.has::<super::ecs::Inactive>(entity) {
                continue;
              }
              if let (Some(line), Some(transform)) = (
                self.world.get_component::<Line2D>(entity),
                self.world.get_component::<Transform>(entity),
              ) {
                trails.add_line(&line, transform.x, transform.y);
              }
            }
          }
          // Only use GPU sprite callback if we have batched sprites or trails
          let has_gpu_sprites = self.batch_collector.sprite_count() > 0;
          let has_trails = trail_renderer
            .as_ref()
            .map(|t| t.vertex_count() > 0)
            .unwrap_or(false);

          // Collect lights from the world
          let lights: Vec<_> = self
            .world
            .query_mut::<(&Transform, &Light2D)>()
            .into_iter()
            .map(|(t, l)| (t.clone(), l.clone()))
            .collect();

          // Collect occluders for shadow casting
          let occluders: Vec<_> = self
            .world
            .query_mut::<(&Transform, &LightOccluder2D)>()
            .into_iter()
            .map(|(t, o)| (t.clone(), o.clone()))
            .collect();

          // Check if any lights cast shadows and there are occluders
          let has_shadow_casting_lights = lights.iter().any(|(_, l)| l.casts_shadows());
          let should_render_shadows = has_shadow_casting_lights && !occluders.is_empty();

          // Auto-enable lighting when Light2D entities exist
          if !lights.is_empty()
            && let Some(ref mut l) = lighting
            && !l.is_enabled()
          {
            l.set_enabled(true);
          }

          // Update god rays effect with positions from lights that contribute
          if let Some(ref l) = lighting {
            let cam_pos = glam::Vec2::new(self.camera.x, self.camera.y);
            l.update_god_rays(&mut self.effects, &lights, cam_pos, self.camera.zoom);
          }

          // Update GPU renderer viewport
          gpu.set_viewport(
            rend.queue(),
            rend.game_width() as f32,
            rend.game_height() as f32,
          );

          // Prepare camera data for trail rendering
          let camera_pos = glam::Vec2::new(self.camera.x, self.camera.y);
          let camera_zoom = self.camera.zoom;
          let time = self
            .time
            .as_ref()
            .map(|t| t.frame_count() as f32 / 60.0)
            .unwrap_or(0.0);

          // Check if lighting is enabled and should be applied
          let lighting_enabled = lighting.as_ref().map(|l| l.is_enabled()).unwrap_or(false);

          let result = if has_gpu_sprites || has_trails || lighting_enabled {
            // Create GPU sprite callback (backgrounds, parallax - renders BEFORE CPU content)
            let batch_collector = &self.batch_collector;
            let gpu_callback = |queue: &wgpu::Queue,
                                encoder: &mut wgpu::CommandEncoder,
                                view: &wgpu::TextureView| {
              if has_gpu_sprites {
                gpu.render_batches(queue, encoder, view, batch_collector);
              }
            };

            // Create trail callback (renders AFTER CPU content so backgrounds don't cover trails)
            let trail_callback = |queue: &wgpu::Queue,
                                  encoder: &mut wgpu::CommandEncoder,
                                  view: &wgpu::TextureView| {
              if let Some(ref mut trails) = trail_renderer {
                trails.render(
                  queue,
                  encoder,
                  view,
                  camera_pos,
                  camera_zoom,
                  time,
                  super::trail::TrailStyle::Solid,
                );
              }
            };

            // Create lighting callback if lighting is enabled
            if lighting_enabled {
              // Borrow lighting for the callback - we can't move it because it needs to be restored
              let lighting_ref = &mut lighting;
              let lighting_callback =
                move |device: &wgpu::Device,
                      queue: &wgpu::Queue,
                      encoder: &mut wgpu::CommandEncoder,
                      scene_texture: &wgpu::Texture,
                      output_view: &wgpu::TextureView| {
                  if let Some(ref mut lighting_renderer) = *lighting_ref {
                    // Render lights to the light accumulation buffer
                    lighting_renderer.render_lights(
                      device,
                      queue,
                      encoder,
                      lights.iter().map(|(t, l)| (t, l)),
                      camera_pos,
                      camera_zoom,
                    );

                    // Render shadows to subtract from light buffer
                    if should_render_shadows {
                      let occluder_refs: Vec<(&Transform, &LightOccluder2D)> =
                        occluders.iter().map(|(t, o)| (t, o)).collect();
                      let shadow_geometry = generate_shadow_geometry(
                        lights.iter().map(|(t, l)| (t, l)),
                        &occluder_refs,
                      );
                      lighting_renderer.render_shadows(device, queue, encoder, &shadow_geometry);
                    }

                    // Compose the lit scene from game texture (has GPU sprites + CPU content)
                    lighting_renderer.compose_from_texture(
                      device,
                      queue,
                      encoder,
                      scene_texture,
                      output_view,
                      time,
                    );
                  }
                };

              // Pass ui_buffer for compositing after lighting
              // If UI was rendered directly to frame, ui_buffer will be zeros (no effect)
              // If UI was rendered separately, it will be composited on top of lit scene
              rend.render_with_lighting(
                Some(&self.effects),
                Some(gpu_callback),
                Some(lighting_callback),
                Some(&self.ui_buffer),
              )
            } else {
              rend.render_with_gpu_sprites(
                Some(&self.effects),
                Some(gpu_callback),
                Some(trail_callback),
                Some(&self.ui_buffer),
              )
            }
          } else {
            rend.render_with_effects(Some(&self.effects), Some(&self.ui_buffer))
          };

          match result {
            Ok(_) => {}
            Err(wgpu::SurfaceError::Outdated) => {
              // Surface needs reconfiguration (display mode changed, window resized, etc.)
              // This is recoverable - the next frame will work after resize event
              self.surface_needs_reconfigure = true;
            }
            Err(wgpu::SurfaceError::Lost) => {
              // Surface was lost, try to reconfigure
              if let Some(ref mut rend) = renderer
                && let Some(window) = &self.window
              {
                let size = window.inner_size();
                rend.resize(size.width, size.height);
              }
            }
            Err(e) => {
              engine_error!(module: "Render", "Render error: {:?}", e);
              // Put back before exiting
              self.sprite_gpu = sprite_gpu;
              self.renderer = renderer;
              self.lighting = lighting;
              self.trail_renderer = trail_renderer;
              event_loop.exit();
              return;
            }
          }
        }

        // Put sprite_gpu, renderer, lighting, and trail_renderer back
        self.sprite_gpu = sprite_gpu;
        self.renderer = renderer;
        self.lighting = lighting;
        self.trail_renderer = trail_renderer;
        // Present time = GPU submission + effects pipeline
        self.frame_timings.present = present_start.elapsed();

        // Track first frame for potential future use
        if !self.first_frame_rendered {
          self.first_frame_rendered = true;
        }

        // Render profiler window directly when game window has focus
        // (Windows won't deliver RedrawRequested to unfocused profiler window)
        #[cfg(feature = "profiler")]
        if self.game_window_focused
          && let Some(ref mut profiler) = self.profiler_window
        {
          profiler.render();
        }

        // Render logger window directly when game window has focus
        // (Windows won't deliver RedrawRequested to unfocused logger window)
        if self.game_window_focused
          && let Some(ref mut logger) = self.logger_window
        {
          logger.render();
        }

        // Calculate total frame time and FPS
        self.frame_timings.total = frame_start.elapsed();
        self.frame_timings.calculate_fps();

        // Debug: Log when a frame takes too long (>33ms = <30fps)
        // Only when profiling is enabled to avoid console spam
        // Debug: Log when a frame takes too long (>33ms = <30fps)
        #[cfg(debug_assertions)]
        {
          let total_ms = self.frame_timings.total.as_secs_f64() * 1000.0;
          if total_ms > 33.0 {
            engine_warn!(
              module: "Perf",
              "SLOW FRAME - Total: {:.1}ms | Update: {:.1}ms | Render: {:.1}ms | Present: {:.1}ms",
              total_ms,
              self.frame_timings.update.as_secs_f64() * 1000.0,
              self.frame_timings.render.as_secs_f64() * 1000.0,
              self.frame_timings.present.as_secs_f64() * 1000.0,
            );
          }
        }

        // Debug output and frame limiting
        if let Some(time) = &mut self.time {
          // Update smoothed timings using actual measured FPS (includes vsync/sleep)
          self
            .smoothed_timings
            .update(&self.frame_timings, time.current_fps());

          let printed_debug = time.print_debug_info();

          // Print frame timing to console periodically (once per second) if enabled
          // This supplements the on-screen overlay for games using GPU sprites
          if self.config.debug.show_frame_timing && printed_debug {
            engine_debug!(
              module: "FrameTiming",
              "Update: {:.2}ms | Render: {:.2}ms | Present: {:.2}ms | Total: {:.2}ms",
              self.frame_timings.update.as_secs_f64() * 1000.0,
              self.frame_timings.render.as_secs_f64() * 1000.0,
              self.frame_timings.present.as_secs_f64() * 1000.0,
              self.frame_timings.total.as_secs_f64() * 1000.0,
            );
          }

          // Frame limiting: only apply manual limiting if presentation doesn't already block
          // AND vsync is not explicitly disabled.
          // - Fifo mode: GPU blocks until vsync, no manual limiting needed
          // - Mailbox/Immediate with vsync on or unset: manual limiting to target_fps
          // - Immediate with vsync=false: uncapped FPS (no limiting)
          let presentation_blocks = self
            .renderer
            .as_ref()
            .map(|r| r.presentation_blocks())
            .unwrap_or(false);

          if should_apply_frame_limiting(presentation_blocks, self.vsync) {
            time.wait_for_target_frame_time();
          }
        }

        // Clear key repeat state every frame so held-key actions (backspace,
        // arrows) don't fire multiple times across consecutive no-update frames
        // when render_ui processes them on each frame.
        self.input.post_render_cleanup();

        // Clear all per-frame input state at the END of the frame, but ONLY if
        // we ran at least one update. This preserves input events for the next
        // frame when the fixed timestep accumulator isn't ready yet.
        if did_update {
          self.input.begin_frame();
        }
      }
      _ => {}
    }
  }

  fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
    // Use Poll mode to keep the event loop running continuously
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);

    // Process any pending window configuration requests
    if self.window_request.has_changes()
      && let Some(window) = self.window.clone()
    {
      self.apply_window_request(&window);
    }

    // Process any pending cursor configuration requests
    if self.cursor_request.has_changes()
      && let Some(window) = self.window.clone()
    {
      self.apply_cursor_request(&window, event_loop);
    }

    // Process pending dev window open requests (F9/F10)
    #[cfg(feature = "profiler")]
    if self.pending_open_profiler
      && self.profiler_window.is_none()
      && let Some(ref renderer) = self.renderer
    {
      self.pending_open_profiler = false;
      // Enable puffin scope collection if not already enabled
      // This allows F10 to work even if enable_profiler wasn't set in config
      puffin::set_scopes_on(true);
      self.profiler_window = super::windows::profiler_window::ProfilerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        renderer.instance(),
        renderer.adapter(),
      );
    }

    if self.pending_open_logger
      && self.logger_window.is_none()
      && let Some(ref renderer) = self.renderer
    {
      self.pending_open_logger = false;
      self.logger_window = super::windows::logger_window::LoggerWindow::new(
        event_loop,
        renderer.device_arc(),
        renderer.queue_arc(),
        renderer.instance(),
        renderer.adapter(),
      );
    }

    // Process pending borderless toggle (F5)
    if self.pending_toggle_borderless {
      self.pending_toggle_borderless = false;
      if let Some(ref window) = self.window {
        match self.config.display_mode {
          DisplayMode::Windowed => {
            // Switch to borderless fullscreen
            self.config.display_mode = DisplayMode::BorderlessFullscreen;
            window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            window.set_decorations(false);
            engine_info!(module: "Window", "Switched to borderless fullscreen");
          }
          DisplayMode::BorderlessFullscreen | DisplayMode::ExclusiveFullscreen => {
            // Switch to windowed
            self.config.display_mode = DisplayMode::Windowed;
            window.set_fullscreen(None);
            window.set_decorations(true);
            engine_info!(module: "Window", "Switched to windowed mode");
          }
        }
        self.surface_needs_reconfigure = true;
      }
    }

    // When game window doesn't have focus, Windows doesn't deliver RedrawRequested events
    // So we manually trigger a frame tick to keep the game running
    // Throttle to 30 FPS only when NO window (game, profiler, or logger) has focus
    if !self.game_window_focused {
      #[cfg(feature = "profiler")]
      let throttle = !self.profiler_window_focused && !self.logger_window_focused;
      #[cfg(not(feature = "profiler"))]
      let throttle = !self.logger_window_focused;

      self.tick_frame(throttle);
    }

    // Only request redraws for windows that have focus
    // (unfocused windows are handled manually in tick_frame to respect frame limiting)
    if self.game_window_focused
      && let Some(window) = &self.window
    {
      window.request_redraw();
    }

    #[cfg(feature = "profiler")]
    if self.profiler_window_focused
      && let Some(ref profiler) = self.profiler_window
    {
      profiler.request_redraw();
    }

    if self.logger_window_focused
      && let Some(ref logger) = self.logger_window
    {
      logger.request_redraw();
    }
  }
}

/// Helper for declaring console commands in [`EngineBuilder::with_console_commands`].
pub struct ConsoleCommandDeclarer<'a> {
  commands: &'a mut Vec<(String, String, Option<String>)>,
}

impl ConsoleCommandDeclarer<'_> {
  /// Declare a console command with a name and description.
  pub fn declare(&mut self, name: impl Into<String>, description: impl Into<String>) {
    self.commands.push((name.into(), description.into(), None));
  }

  /// Declare a console command with a name, description, and verbose help text.
  pub fn declare_verbose(
    &mut self,
    name: impl Into<String>,
    description: impl Into<String>,
    verbose_description: impl Into<String>,
  ) {
    self.commands.push((
      name.into(),
      description.into(),
      Some(verbose_description.into()),
    ));
  }
}

/// Builder pattern for Engine configuration
pub struct EngineBuilder {
  config: EngineConfig,
  game: Option<Box<dyn Game>>,
  vsync: Option<bool>,
  master_volume: f32,
  music_volume: f32,
  sfx_volume: f32,
  /// Tracks whether resolution was explicitly set via .resolution() (prevents build() from overwriting)
  resolution_set: bool,
  /// Tracks whether display_mode was explicitly set (prevents build() from overwriting)
  display_mode_set: bool,
  /// Whether to disable F1-F4 debug key toggles
  disable_debug_keys: bool,
  /// Pre-declared console commands (name, description, verbose_description)
  console_commands: Vec<(String, String, Option<String>)>,
}

impl EngineBuilder {
  pub fn new() -> Self {
    EngineBuilder {
      config: EngineConfig::default(),
      game: None,
      vsync: None,
      master_volume: 1.0,
      music_volume: 0.8,
      sfx_volume: 1.0,
      resolution_set: false,
      display_mode_set: false,
      disable_debug_keys: false,
      console_commands: Vec::new(),
    }
  }

  pub fn title(mut self, title: impl Into<String>) -> Self {
    self.config.title = title.into();
    self
  }

  pub fn resolution(mut self, width: u32, height: u32) -> Self {
    self.config.width = width;
    self.config.height = height;
    self.resolution_set = true;
    self
  }

  pub fn fps(mut self, fps: u32) -> Self {
    self.config.target_fps = fps;
    self
  }

  pub fn physics_hz(mut self, hz: u32) -> Self {
    self.config.physics_hz = hz;
    self
  }

  /// Set the window resize behavior.
  ///
  /// Controls how the game viewport responds to window resizing, maximizing,
  /// and drag-to-resize operations.
  ///
  /// # Modes
  ///
  /// - [`WindowMode::Fixed`] - Window cannot be resized
  /// - [`WindowMode::ResizableAspectRatio`] - Scale with letterboxing (default)
  /// - [`WindowMode::ResizableIntegerScale`] - Integer scaling for pixel-perfect
  /// - [`WindowMode::ResizableStretch`] - Stretch to fill (may distort)
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Pixel Art Game")
  ///     .resolution(320, 240)
  ///     .window_mode(WindowMode::ResizableIntegerScale)
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn window_mode(mut self, mode: WindowMode) -> Self {
    self.config.window_mode = mode;
    self
  }

  /// Set the display mode (windowed, fullscreen, or borderless).
  ///
  /// Controls whether the game runs in a window or takes over the screen.
  ///
  /// # Modes
  ///
  /// - [`DisplayMode::Windowed`] - Regular window (default)
  /// - [`DisplayMode::ExclusiveFullscreen`] - Exclusive fullscreen (takes over display)
  /// - [`DisplayMode::BorderlessFullscreen`] - Borderless window filling screen
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .resolution(1920, 1080)
  ///     .display_mode(DisplayMode::BorderlessFullscreen)
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn display_mode(mut self, mode: DisplayMode) -> Self {
    self.config.display_mode = mode;
    self.display_mode_set = true;
    self
  }

  /// Set maximum width for letterboxed viewport.
  ///
  /// Constrains how much the game viewport can scale up horizontally in
  /// letterbox/pillarbox modes. Useful for maintaining pixel density or
  /// limiting blur on large displays.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Pixel Art Game")
  ///     .resolution(320, 240)
  ///     .max_width(1280)  // Don't scale wider than 1280px
  ///     .window_mode(WindowMode::ResizableAspectRatio)
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn max_width(mut self, max: u32) -> Self {
    self.config.max_width = Some(max);
    self
  }

  /// Set maximum height for letterboxed viewport.
  ///
  /// Constrains how much the game viewport can scale up vertically in
  /// letterbox/pillarbox modes. Useful for maintaining pixel density or
  /// limiting blur on large displays.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Pixel Art Game")
  ///     .resolution(320, 240)
  ///     .max_height(960)  // Don't scale taller than 960px
  ///     .window_mode(WindowMode::ResizableAspectRatio)
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn max_height(mut self, max: u32) -> Self {
    self.config.max_height = Some(max);
    self
  }

  /// Set the UI scale factor for HiDPI displays.
  ///
  /// Controls the size of UI text and elements. Use integer values for
  /// crisp pixel-art scaling (nearest-neighbor interpolation).
  ///
  /// - `None` (default) - Auto-detect from window DPI scale factor
  /// - `Some(1)` - Normal size (good for standard displays)
  /// - `Some(2)` - Double size (good for HiDPI/Retina displays)
  /// - `Some(3)` - Triple size (for very high DPI or large fonts)
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// // Force 2x UI scaling regardless of DPI
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .resolution(800, 600)
  ///     .ui_scale(Some(2))
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  ///
  /// // Auto-detect from system DPI (default behavior)
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .resolution(800, 600)
  ///     .ui_scale(None)
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn ui_scale(mut self, scale: Option<u32>) -> Self {
    self.config.ui_scale = scale;
    self
  }

  /// Set a custom window icon from a PNG file.
  ///
  /// The icon appears in the window title bar and taskbar. If not specified,
  /// the default Ryo2D logo is used.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .icon("assets/icon.png")
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn icon(mut self, path: impl Into<std::path::PathBuf>) -> Self {
    self.config.icon_path = Some(path.into());
    self
  }

  /// Enable or disable collision boundary debug visualization.
  ///
  /// When enabled, renders bright green outlines around all colliders
  /// to help debug collision detection issues.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Debug Game")
  ///     .show_colliders(true)
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn show_colliders(mut self, enabled: bool) -> Self {
    self.config.debug.show_colliders = enabled;
    self
  }

  /// Enable or disable FPS counter display.
  ///
  /// When enabled, renders an FPS counter in the top-left corner.
  pub fn show_fps(mut self, enabled: bool) -> Self {
    self.config.debug.show_fps = enabled;
    self
  }

  /// Enable or disable velocity vector visualization.
  ///
  /// When enabled, renders cyan arrows showing entity velocities.
  pub fn show_velocities(mut self, enabled: bool) -> Self {
    self.config.debug.show_velocities = enabled;
    self
  }

  /// Enable or disable entity origin visualization.
  ///
  /// When enabled, renders yellow crosshairs at entity transform positions.
  pub fn show_origins(mut self, enabled: bool) -> Self {
    self.config.debug.show_origins = enabled;
    self
  }

  /// Load user preferences from system.json.
  ///
  /// Loads saved player settings from `system.json`, overriding Ryo.toml defaults.
  /// Use this when you want players to be able to customize and save their preferences.
  ///
  /// # Priority Chain
  ///
  /// 1. **system.json** (user preferences) - loaded by this method
  /// 2. **Ryo.toml `[system]`** (developer defaults) - auto-applied in build()
  /// 3. **Engine defaults** - fallback
  ///
  /// If system.json doesn't exist, it will be created from Ryo.toml defaults.
  ///
  /// # Field Filtering
  ///
  /// Use a [`UserPreferenceFilter`](super::config::UserPreferenceFilter) to control which
  /// preferences are loaded. Fields not in the filter use Ryo.toml defaults.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use ryo2d::prelude::*;
  ///
  /// // Load all user preferences
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_user_preferences(UserPreferenceFilter::all())
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  ///
  /// // Load only volume preferences (ignore display settings)
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_user_preferences([UserPreference::MasterVolume, UserPreference::MusicVolume, UserPreference::SfxVolume])
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  ///
  /// // Don't load any user preferences (use only Ryo.toml defaults)
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_user_preferences(UserPreferenceFilter::none())
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_user_preferences(
    mut self,
    filter: impl Into<super::config::UserPreferenceFilter>,
  ) -> Self {
    use super::config::UserPreference;

    let filter = filter.into();

    if let Ok(workspace) = super::config::Workspace::discover()
      && let Ok(save_manager) = workspace.create_save_manager()
      && let Ok(system_state) =
        save_manager.load_or_create_system_filtered(&workspace.config().system, &filter)
    {
      // Apply display overrides based on filter (only if not explicitly set via .resolution())
      if filter.contains(UserPreference::Resolution) && !self.resolution_set {
        self.config.width = system_state.display.width;
        self.config.height = system_state.display.height;
        self.resolution_set = true;
      }
      if filter.contains(UserPreference::DisplayMode) {
        self.config.display_mode = system_state.display.display_mode;
        self.display_mode_set = true;
      }

      // Apply show_fps if present and in filter
      if filter.contains(UserPreference::ShowFps)
        && let Some(show_fps) = system_state.show_fps
      {
        self.config.debug.show_fps = show_fps;
      }

      // Apply vsync if in filter
      if filter.contains(UserPreference::Vsync) {
        self.vsync = system_state.vsync;
      }

      // Apply audio volumes based on filter
      if filter.contains(UserPreference::MasterVolume) {
        self.master_volume = system_state.master_volume;
      }
      if filter.contains(UserPreference::MusicVolume) {
        self.music_volume = system_state.music_volume;
      }
      if filter.contains(UserPreference::SfxVolume) {
        self.sfx_volume = system_state.sfx_volume;
      }
    }
    self
  }

  /// Apply only display mode from system.json, keeping fixed resolution.
  ///
  /// This is useful for games with a fixed design resolution (e.g., pixel art games)
  /// that still want to respect the user's display mode preference (windowed/fullscreen).
  ///
  /// Unlike [`with_user_preferences`](Self::with_user_preferences), this method
  /// does NOT override the game's resolution - only the display mode and audio/vsync settings.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Pixel Art Game")
  ///     .resolution(320, 240)  // Fixed design resolution
  ///     .with_saved_display_mode()  // Load windowed/fullscreen from user settings
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_saved_display_mode(mut self) -> Self {
    use super::config::{UserPreference, UserPreferenceFilter};

    // Everything except Resolution — this method keeps the game's fixed resolution
    let filter = UserPreferenceFilter::only([
      UserPreference::DisplayMode,
      UserPreference::Vsync,
      UserPreference::ShowFps,
      UserPreference::MasterVolume,
      UserPreference::MusicVolume,
      UserPreference::SfxVolume,
    ]);

    if let Ok(workspace) = super::config::Workspace::discover()
      && let Ok(save_manager) = workspace.create_save_manager()
      && let Ok(system_state) =
        save_manager.load_or_create_system_filtered(&workspace.config().system, &filter)
    {
      // Apply ONLY display mode - not resolution
      self.config.display_mode = system_state.display.display_mode;
      self.display_mode_set = true;

      // Apply overrides if present
      if let Some(show_fps) = system_state.show_fps {
        self.config.debug.show_fps = show_fps;
      }
      // Store vsync preference to use when creating renderer
      // Falls back to display.vsync when root vsync is None
      self.vsync = system_state.vsync.or(Some(system_state.display.vsync));
      // Store audio volumes to apply after AudioManager creation
      self.master_volume = system_state.master_volume;
      self.music_volume = system_state.music_volume;
      self.sfx_volume = system_state.sfx_volume;
    }
    self
  }

  /// Declare console commands at engine startup.
  ///
  /// Commands declared here are available immediately when the engine starts,
  /// before any scene's `on_enter` runs. This is the recommended place for
  /// game-wide commands like `game:reset`, `game:godmode`, or `game:debug`.
  ///
  /// Scene-specific commands can still be declared via
  /// [`SceneContext::declare_console_command`] in `on_enter`.
  ///
  /// To attach a handler to a declared command, use
  /// [`SceneContext::register_console_command`] in a scene's `on_enter`.
  /// Handlers are automatically cleaned up when the scene exits.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_console_commands(|cmd| {
  ///         cmd.declare("game:reset", "Reset the game to initial state");
  ///         cmd.declare("game:godmode", "Toggle invincibility");
  ///         cmd.declare_verbose(
  ///             "game:spawn",
  ///             "Spawn an entity",
  ///             "Usage: game:spawn <enemy|item> [x] [y]",
  ///         );
  ///     })
  ///     .with_scene(MenuScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_console_commands(mut self, f: impl FnOnce(&mut ConsoleCommandDeclarer)) -> Self {
    let mut declarer = ConsoleCommandDeclarer {
      commands: &mut self.console_commands,
    };
    f(&mut declarer);
    self
  }

  /// Set the game implementation that will run on this engine.
  ///
  /// Use this for simple games or quick prototypes where you don't need
  /// scene management. For games with multiple screens (menu, gameplay,
  /// pause), use [`with_scene`](Self::with_scene) instead.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Simple Game")
  ///     .with_game(MyGame::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_game(mut self, game: impl Game + 'static) -> Self {
    self.game = Some(Box::new(game));
    self
  }

  /// Set the initial scene that will run on this engine.
  ///
  /// Use this for games that need scene management (menus, pause screens,
  /// level transitions). Scenes can push/pop other scenes, enabling
  /// complex game flows.
  ///
  /// For simple games without scene transitions, you can use
  /// [`with_game`](Self::with_game) instead.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("Scene-Based Game")
  ///     .with_scene(MenuScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_scene(mut self, scene: impl Scene + 'static) -> Self {
    self.game = Some(Box::new(SceneGame::new(Box::new(scene))));
    self
  }

  pub fn build(self) -> Engine {
    let game = self.game.expect(
      "EngineBuilder requires a game or scene - call .with_game() or .with_scene() before .build()",
    );

    let mut config = self.config;
    let mut vsync = self.vsync;
    let mut master_volume = self.master_volume;
    let mut music_volume = self.music_volume;
    let mut sfx_volume = self.sfx_volume;
    let resolution_set = self.resolution_set;
    let display_mode_set = self.display_mode_set;
    let mut disable_debug_keys = self.disable_debug_keys;
    let mut game_name: Option<String> = None;
    let mut game_version: Option<String> = None;

    // Priority chain for configuration:
    // 1. Explicit builder calls (highest) - already in self.config
    // 2. Embedded values from `ryo2d build` (compile-time)
    // 3. Runtime workspace discovery (Ryo.toml)
    // 4. Engine defaults (lowest) - already in EngineConfig::default()

    // Check for embedded values (set when built via `ryo2d build`)
    let has_embedded = option_env!("RYO_EMBED_RESOLUTION").is_some();

    if has_embedded {
      // Use compile-time embedded values from Ryo.toml
      if !resolution_set && let Some(res) = option_env!("RYO_EMBED_RESOLUTION") {
        if let Some((w, h)) = super::config::save::parse_resolution(res) {
          config.width = w;
          config.height = h;
        } else {
          eprintln!(
            "[ryo2d] Warning: Malformed embedded resolution '{}', using engine defaults",
            res
          );
        }
      }
      if let Some(v) = option_env!("RYO_EMBED_VSYNC") {
        vsync = Some(v.parse().unwrap_or(true));
      }
      if let Some(fps) = option_env!("RYO_EMBED_SHOW_FPS") {
        config.debug.show_fps = fps.parse().unwrap_or(false);
      }
      // [debug] section embedded values
      if let Some(v) = option_env!("RYO_EMBED_SHOW_COLLIDERS") {
        config.debug.show_colliders = v.parse().unwrap_or(false);
      }
      if let Some(v) = option_env!("RYO_EMBED_SHOW_VELOCITIES") {
        config.debug.show_velocities = v.parse().unwrap_or(false);
      }
      if let Some(v) = option_env!("RYO_EMBED_SHOW_ORIGINS") {
        config.debug.show_origins = v.parse().unwrap_or(false);
      }
      if let Some(v) = option_env!("RYO_EMBED_SHOW_FRAME_TIMING") {
        config.debug.show_frame_timing = v.parse().unwrap_or(false);
      }
      if let Some(v) = option_env!("RYO_EMBED_DISABLE_DEBUG_KEYS") {
        disable_debug_keys = v.parse().unwrap_or(false);
      }
      if let Some(v) = option_env!("RYO_EMBED_ENABLE_PROFILER") {
        config.debug.enable_profiler = v.parse().unwrap_or(false);
      }
      if let Some(vol) = option_env!("RYO_EMBED_MASTER_VOLUME") {
        master_volume = vol.parse().unwrap_or(0.7);
      }
      if let Some(vol) = option_env!("RYO_EMBED_MUSIC_VOLUME") {
        music_volume = vol.parse().unwrap_or(1.0);
      }
      if let Some(vol) = option_env!("RYO_EMBED_SFX_VOLUME") {
        sfx_volume = vol.parse().unwrap_or(1.0);
      }
      if let Some(title) = option_env!("RYO_EMBED_TITLE")
        && config.title == "Ryo2D"
      {
        config.title = title.to_string();
      }
      // Display mode (only if not explicitly set via builder or system.json)
      if !display_mode_set && let Some(mode) = option_env!("RYO_EMBED_DISPLAY_MODE") {
        config.display_mode = match mode.to_lowercase().as_str() {
          "exclusive_fullscreen" => DisplayMode::ExclusiveFullscreen,
          "borderless_fullscreen" => DisplayMode::BorderlessFullscreen,
          _ => DisplayMode::Windowed,
        };
      }
      // Developer console (from [system] section)
      if let Some(v) = option_env!("RYO_EMBED_CONSOLE") {
        config.debug.dev_console = v.parse().unwrap_or(false);
      }
    } else if let Ok(workspace) = super::config::Workspace::discover() {
      // Fall back to runtime workspace discovery (development mode)
      let wconfig = workspace.config();
      let debug = &wconfig.debug;
      let system = &wconfig.system;
      let release = &wconfig.release;

      // Load game info from [project] section
      game_name = Some(wconfig.name.clone());
      game_version = wconfig.version.clone();

      // Load window title from [project].title (only if not explicitly set via builder)
      if config.title == "Ryo2D" {
        config.title = wconfig.title.clone();
      }

      // Load debug settings
      config.debug.show_colliders = debug.show_colliders;
      config.debug.show_fps = system.show_fps; // From [system] section
      config.debug.show_velocities = debug.show_velocities;
      config.debug.show_origins = debug.show_origins;
      config.debug.show_frame_timing = debug.show_frame_timing;
      config.debug.enable_profiler = debug.enable_profiler;
      config.debug.logger_window = debug.logger_window;
      config.debug.dev_window_keys = debug.dev_window_keys;
      config.debug.dev_console = system.dev_console; // From [system] section for release build support
      disable_debug_keys = debug.disable_debug_keys;

      // Load resolution from [system] section if not explicitly set via builder
      if !resolution_set && let Some((w, h)) = system.resolution {
        config.width = w;
        config.height = h;
      }

      // Load display mode from [system] section (only if not explicitly set)
      if !display_mode_set {
        config.display_mode = system.display_mode;
      }

      // Load audio volumes from [system] section
      master_volume = system.master_volume;
      music_volume = system.music_volume;
      sfx_volume = system.sfx_volume;

      // Load vsync from [system] section (only if not explicitly set)
      if vsync.is_none() {
        vsync = Some(system.vsync);
      }

      // Load icon from [release] section if specified and not explicitly set via builder
      if config.icon_path.is_none()
        && let Some(logo_path) = &release.logo
      {
        // Make path absolute relative to workspace root
        config.icon_path = Some(workspace.root().join(logo_path));
      }
    }

    // Runtime env var override for profiler (from `ryo2d dev --profiler`)
    // When -p flag is used, ensure enable_profiler is true so puffin scopes are collected
    if std::env::var("RYO_PROFILER_AUTO_OPEN").is_ok() {
      config.debug.enable_profiler = true;
    }

    // For release builds: check for icon.png next to executable (created by `ryo2d build`)
    // If not found, load_window_icon() will use the embedded default Ryo2D logo
    if config.icon_path.is_none()
      && let Ok(exe_path) = std::env::current_exe()
      && let Some(exe_dir) = exe_path.parent()
    {
      let icon_path = exe_dir.join("icon.png");
      if icon_path.exists() {
        config.icon_path = Some(icon_path);
      }
    }

    Engine::new(
      config,
      game,
      vsync,
      master_volume,
      music_volume,
      sfx_volume,
      disable_debug_keys,
      game_name,
      game_version,
      self.console_commands,
    )
  }
}

impl Default for EngineBuilder {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn engine_config_has_sensible_defaults() {
    let config = EngineConfig::default();
    assert_eq!(config.title, "Ryo2D");
    assert_eq!(config.width, 1280);
    assert_eq!(config.height, 720);
    assert_eq!(config.target_fps, 60);
    assert_eq!(config.physics_hz, 60);
  }

  #[test]
  fn engine_builder_sets_title() {
    let builder = EngineBuilder::new().title("Test Game");
    assert_eq!(builder.config.title, "Test Game");
  }

  #[test]
  fn engine_builder_sets_resolution() {
    let builder = EngineBuilder::new().resolution(1920, 1080);
    assert_eq!(builder.config.width, 1920);
    assert_eq!(builder.config.height, 1080);
  }

  #[test]
  fn engine_builder_sets_fps() {
    let builder = EngineBuilder::new().fps(144);
    assert_eq!(builder.config.target_fps, 144);
  }

  #[test]
  fn engine_builder_sets_physics_hz() {
    let builder = EngineBuilder::new().physics_hz(120);
    assert_eq!(builder.config.physics_hz, 120);
  }

  #[test]
  fn engine_builder_chains() {
    let builder = EngineBuilder::new()
      .title("Chained")
      .resolution(640, 480)
      .fps(30)
      .physics_hz(50);

    assert_eq!(builder.config.title, "Chained");
    assert_eq!(builder.config.width, 640);
    assert_eq!(builder.config.height, 480);
    assert_eq!(builder.config.target_fps, 30);
    assert_eq!(builder.config.physics_hz, 50);
  }

  #[test]
  fn window_mode_defaults_to_resizable_aspect_ratio() {
    let mode = WindowMode::default();
    assert_eq!(mode, WindowMode::ResizableAspectRatio);
  }

  #[test]
  fn engine_config_includes_window_mode() {
    let config = EngineConfig::default();
    assert_eq!(config.window_mode, WindowMode::ResizableAspectRatio);
  }

  #[test]
  fn engine_builder_sets_window_mode() {
    let builder = EngineBuilder::new().window_mode(WindowMode::Fixed);
    assert_eq!(builder.config.window_mode, WindowMode::Fixed);

    let builder = EngineBuilder::new().window_mode(WindowMode::ResizableIntegerScale);
    assert_eq!(
      builder.config.window_mode,
      WindowMode::ResizableIntegerScale
    );

    let builder = EngineBuilder::new().window_mode(WindowMode::ResizableStretch);
    assert_eq!(builder.config.window_mode, WindowMode::ResizableStretch);
  }

  // ===========================================================================
  // Debug Keys Tests
  // ===========================================================================

  #[test]
  fn engine_builder_debug_keys_enabled_by_default() {
    let builder = EngineBuilder::new();
    assert!(!builder.disable_debug_keys);
  }

  #[test]
  fn overlay_mode_syncs_to_debug_config_flags() {
    // Test that OverlayMode correctly maps to config flags
    let mode = OverlayMode::Off;
    assert_eq!(mode.to_flags(), (false, false, false));

    let mode = OverlayMode::FpsOnly;
    assert_eq!(mode.to_flags(), (true, false, false));

    // Frame timing includes FPS, so show_fps is false
    let mode = OverlayMode::FrameTiming;
    assert_eq!(mode.to_flags(), (false, true, false));

    let mode = OverlayMode::FullStats;
    assert_eq!(mode.to_flags(), (false, true, true));
  }

  #[test]
  fn overlay_mode_from_debug_config_flags() {
    // Test that config flags correctly map to OverlayMode
    assert_eq!(
      OverlayMode::from_flags(false, false, false),
      OverlayMode::Off
    );
    assert_eq!(
      OverlayMode::from_flags(true, false, false),
      OverlayMode::FpsOnly
    );
    // Frame timing mode (show_fps ignored when frame_timing is true)
    assert_eq!(
      OverlayMode::from_flags(false, true, false),
      OverlayMode::FrameTiming
    );
    assert_eq!(
      OverlayMode::from_flags(false, true, true),
      OverlayMode::FullStats
    );
  }

  // ===========================================================================
  // Frame Limiting / VSync Tests
  // ===========================================================================

  #[test]
  fn frame_limiting_applied_when_vsync_not_set() {
    // Default behavior: limit to target_fps when vsync is not configured
    assert!(should_apply_frame_limiting(false, None));
  }

  #[test]
  fn frame_limiting_applied_when_vsync_on() {
    // VSync on with Mailbox mode: still apply frame limiting
    assert!(should_apply_frame_limiting(false, Some(true)));
  }

  #[test]
  fn frame_limiting_skipped_when_vsync_off() {
    // VSync explicitly off: uncapped FPS, no frame limiting
    assert!(!should_apply_frame_limiting(false, Some(false)));
  }

  #[test]
  fn frame_limiting_skipped_when_presentation_blocks() {
    // Fifo mode blocks on presentation: no manual limiting needed
    assert!(!should_apply_frame_limiting(true, None));
    assert!(!should_apply_frame_limiting(true, Some(true)));
    assert!(!should_apply_frame_limiting(true, Some(false)));
  }

  #[test]
  fn engine_builder_vsync_defaults_to_none() {
    let builder = EngineBuilder::new();
    assert_eq!(builder.vsync, None);
  }

  // ===========================================================================
  // Console Command Declaration Tests
  // ===========================================================================

  #[test]
  fn engine_builder_console_commands_empty_by_default() {
    let builder = EngineBuilder::new();
    assert!(builder.console_commands.is_empty());
  }

  #[test]
  fn engine_builder_with_console_commands_declares_simple() {
    let builder = EngineBuilder::new().with_console_commands(|cmd| {
      cmd.declare("game:reset", "Reset the game");
    });
    assert_eq!(builder.console_commands.len(), 1);
    assert_eq!(builder.console_commands[0].0, "game:reset");
    assert_eq!(builder.console_commands[0].1, "Reset the game");
    assert!(builder.console_commands[0].2.is_none());
  }

  #[test]
  fn engine_builder_with_console_commands_declares_verbose() {
    let builder = EngineBuilder::new().with_console_commands(|cmd| {
      cmd.declare_verbose("game:spawn", "Spawn an entity", "Usage: game:spawn <type> [x] [y]");
    });
    assert_eq!(builder.console_commands.len(), 1);
    assert_eq!(builder.console_commands[0].0, "game:spawn");
    assert_eq!(builder.console_commands[0].1, "Spawn an entity");
    assert_eq!(
      builder.console_commands[0].2.as_deref(),
      Some("Usage: game:spawn <type> [x] [y]")
    );
  }

  #[test]
  fn engine_builder_with_console_commands_multiple() {
    let builder = EngineBuilder::new().with_console_commands(|cmd| {
      cmd.declare("game:reset", "Reset the game");
      cmd.declare("game:godmode", "Toggle invincibility");
      cmd.declare_verbose("game:spawn", "Spawn an entity", "Usage: game:spawn <type>");
    });
    assert_eq!(builder.console_commands.len(), 3);
    assert_eq!(builder.console_commands[0].0, "game:reset");
    assert_eq!(builder.console_commands[1].0, "game:godmode");
    assert_eq!(builder.console_commands[2].0, "game:spawn");
  }

  #[test]
  fn engine_builder_with_console_commands_chains_with_other_builder_methods() {
    let builder = EngineBuilder::new()
      .title("Test Game")
      .resolution(800, 600)
      .with_console_commands(|cmd| {
        cmd.declare("game:reset", "Reset");
      })
      .fps(60);
    assert_eq!(builder.config.title, "Test Game");
    assert_eq!(builder.config.width, 800);
    assert_eq!(builder.console_commands.len(), 1);
    assert_eq!(builder.config.target_fps, 60);
  }
}
