use std::sync::Arc;
use std::time::{Duration, Instant};

use gilrs::Gilrs;
use winit::application::ApplicationHandler;
use winit::event::{Ime, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use super::asset::AssetManager;
use super::audio::AudioManager;
use super::config::{self as debug, DebugConfig, DisplayMode, FrameTimings, SmoothedFrameTimings};
use super::ecs::{
  Entity, RenderStyle, Renderable, SpriteShader, Transform, Tween, Velocity, World, WorldTransform,
};
use super::error::{EngineError, EngineResult};
use super::events::{CollisionStarted, CollisionStopped, Events};
use super::game::{Game, GameContext, GpuRenderContext, WindowRequest};
use super::graphics::{
  AnimationController, Animator, BatchCollector, Camera, CompositeTile, GroupedCollider, Sprite,
  SpriteGpuRenderer, TileDirection, WgpuRenderer,
};
use super::input::InputState;
use super::lighting::{Light2D, LightingRenderer};
use super::physics::{Collider, PhysicsConfig, RapierPhysicsWorld, RigidBody};
use super::scene::{Scene, SceneGame};
use super::shaders::EffectsManager;
use super::trail::{Line2D, Trail2D, TrailRenderer};
use super::ui::{BitmapFont, UiContext, create_debug_font};

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

/// Tracks timing state for the game loop
#[derive(Debug)]
struct TimeState {
  /// When the current frame started
  frame_start: Instant,
  /// Time since last frame (variable)
  delta_time: Duration,
  /// Fixed timestep for physics/logic updates
  fixed_timestep: Duration,
  /// Accumulator for fixed timestep updates
  accumulator: Duration,
  /// Target frame duration for FPS limiting
  target_frame_time: Duration,
  /// Frame counter for debugging
  frame_count: u64,
  /// Last calculated FPS (updated each second)
  current_fps: u64,
  /// Last time we printed debug info
  last_debug_print: Instant,
}

impl TimeState {
  fn new(target_fps: u32, physics_hz: u32) -> Self {
    let now = Instant::now();
    TimeState {
      frame_start: now,
      delta_time: Duration::ZERO,
      fixed_timestep: Duration::from_secs_f64(1.0 / physics_hz as f64),
      accumulator: Duration::ZERO,
      target_frame_time: Duration::from_secs_f64(1.0 / target_fps as f64),
      frame_count: 0,
      current_fps: 0,
      last_debug_print: now,
    }
  }

  fn begin_frame(&mut self) {
    let now = Instant::now();
    self.delta_time = now.duration_since(self.frame_start);
    self.frame_start = now;

    // Add delta to accumulator, but cap it to prevent spiral of death
    // If we fall too far behind, we just drop updates rather than freezing
    self.accumulator += self.delta_time;
    let max_accumulator = self.fixed_timestep * 5; // Max 5 physics updates per frame
    if self.accumulator > max_accumulator {
      self.accumulator = max_accumulator;
    }

    self.frame_count += 1;
  }

  fn should_update(&self) -> bool {
    self.accumulator >= self.fixed_timestep
  }

  fn consume_update(&mut self) {
    self.accumulator -= self.fixed_timestep;
  }

  #[allow(dead_code)]
  fn dt(&self) -> f32 {
    self.delta_time.as_secs_f32()
  }

  fn fixed_dt(&self) -> f32 {
    self.fixed_timestep.as_secs_f32()
  }

  fn interpolation_alpha(&self) -> f32 {
    (self.accumulator.as_secs_f32() / self.fixed_timestep.as_secs_f32()).min(1.0)
  }

  fn print_debug_info(&mut self) -> bool {
    let now = Instant::now();
    if now.duration_since(self.last_debug_print) >= Duration::from_secs(1) {
      self.current_fps = self.frame_count;
      println!(
        "[GameLoop] FPS: {}, Delta: {:.2}ms, Fixed DT: {:.2}ms",
        self.current_fps,
        self.delta_time.as_secs_f64() * 1000.0,
        self.fixed_timestep.as_secs_f64() * 1000.0
      );
      self.frame_count = 0;
      self.last_debug_print = now;
      true // Signal that debug info was printed
    } else {
      false
    }
  }

  /// Wait precisely until target frame time using hybrid sleep/spin.
  ///
  /// This is more accurate than pure `thread::sleep()` which has ~1-15ms
  /// granularity depending on OS scheduler. We sleep for most of the time
  /// to save CPU, then spin-wait for the final microseconds for precision.
  fn wait_for_target_frame_time(&self) {
    let elapsed = self.frame_start.elapsed();
    if elapsed >= self.target_frame_time {
      return; // Already past target, no waiting needed
    }

    let remaining = self.target_frame_time - elapsed;

    // Sleep for most of the remaining time (save CPU)
    // Leave 1.5ms margin for spin-waiting to hit exact target
    let spin_margin = Duration::from_micros(1500);
    if remaining > spin_margin {
      std::thread::sleep(remaining - spin_margin);
    }

    // Spin-wait for precise timing (busy loop but very short)
    while self.frame_start.elapsed() < self.target_frame_time {
      std::hint::spin_loop();
    }
  }
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
}

/// Re-export for backward compatibility
pub type EngineDebugConfig = DebugConfig;

impl Default for EngineConfig {
  fn default() -> Self {
    EngineConfig {
      title: String::from("Ryo2D"),
      width: 800,
      height: 600,
      target_fps: 60,
      physics_hz: 60,
      physics: PhysicsConfig::default(),
      debug: EngineDebugConfig::default(),
      window_mode: WindowMode::default(),
      display_mode: DisplayMode::default(),
      max_width: None,
      max_height: None,
    }
  }
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
  // Whether debug font has been uploaded to GPU
  debug_font_uploaded: bool,
  // Temporary buffer for UI when not affected by lighting
  ui_buffer: Vec<u8>,
  // Pending window configuration changes from game
  window_request: WindowRequest,
  // Flag indicating surface needs reconfiguration (after display mode change)
  surface_needs_reconfigure: bool,
  // Event bus for decoupled inter-system communication
  events: Events,
  // Gamepad input library (gilrs)
  gilrs: Option<Gilrs>,
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
        DisplayMode::Fullscreen => {
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

    // Clear the request after processing
    self.window_request.clear();
  }

  fn new(
    config: EngineConfig,
    game: Box<dyn Game>,
    vsync: Option<bool>,
    master_volume: f32,
    music_volume: f32,
    sfx_volume: f32,
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
          eprintln!("[HotReload] Failed to enable: {}", e);
        }
      }
    }

    // Try to initialize audio (gracefully handle no audio device)
    let mut audio = match AudioManager::new() {
      Ok(manager) => Some(manager),
      Err(e) => {
        eprintln!(
          "Warning: Could not initialize audio: {}. Continuing without sound.",
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
        println!("[Gamepad] Initialized successfully");
        Some(g)
      }
      Err(e) => {
        eprintln!(
          "Warning: Could not initialize gamepad support: {}. Continuing without gamepads.",
          e
        );
        None
      }
    };

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
      debug_font_uploaded: false,
      ui_buffer: vec![0; ui_buffer_size], // RGBA buffer for UI
      window_request: WindowRequest::default(),
      surface_needs_reconfigure: false,
      events: Events::new(),
      gilrs,
    }
  }

  // =============================================================================
  // Main Loop
  // =============================================================================

  /// Run the engine
  pub fn run(mut self) -> EngineResult<()> {
    println!("Initializing {}...", self.config.title);
    println!("  Resolution: {}x{}", self.config.width, self.config.height);
    println!("  Target FPS: {}", self.config.target_fps);

    let event_loop = EventLoop::new().map_err(|e| EngineError::Window(format!("{}", e)))?;

    event_loop
      .run_app(&mut self)
      .map_err(|e| EngineError::Window(format!("Event loop error: {}", e)))
  }

  // =============================================================================
  // Update System - Game Logic & Physics
  // =============================================================================

  /// Fixed timestep update - called multiple times per frame if needed
  ///
  /// This runs user game logic and built-in engine systems (physics, movement, camera).
  fn update(&mut self, _dt: f32) {
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
    // Update all Animator components
    for entity in self.world.entities_with::<Animator>() {
      if let Some(mut animator) = self.world.get_component_mut::<Animator>(entity) {
        animator.update(_dt);
      }
    }

    // Sync Animator sprites to Renderable
    for entity in self.world.entities_with::<Animator>() {
      let sprite = self
        .world
        .get_component::<Animator>(entity)
        .and_then(|a| a.current_sprite().cloned());
      if let Some(sprite) = sprite
        && let Some(mut renderable) = self.world.get_component_mut::<Renderable>(entity)
      {
        renderable.sprite = sprite;
      }
    }

    // Update all AnimationController components
    for entity in self.world.entities_with::<AnimationController>() {
      if let Some(mut controller) = self.world.get_component_mut::<AnimationController>(entity) {
        controller.update(_dt);
      }
    }

    // Sync AnimationController sprites to Renderable
    for entity in self.world.entities_with::<AnimationController>() {
      let sprite = self
        .world
        .get_component::<AnimationController>(entity)
        .and_then(|c| c.current_sprite().cloned());
      if let Some(sprite) = sprite
        && let Some(mut renderable) = self.world.get_component_mut::<Renderable>(entity)
      {
        renderable.sprite = sprite;
      }
    }

    // --- Tween System ---
    // Update all Tween components and apply to Transform
    for entity in self.world.entities_with::<Tween>() {
      // Check if tween is playing before borrowing mutably
      let should_update = self
        .world
        .get_component::<Tween>(entity)
        .is_some_and(|t| t.playing);

      if !should_update {
        continue;
      }

      // Get transform and tween together for update
      if let (Some(mut transform), Some(mut tween)) = (
        self.world.get_component_mut::<Transform>(entity),
        self.world.get_component_mut::<Tween>(entity),
      ) {
        tween.update(_dt, &mut transform);
        // Note: Tweens only modify Transform (position/scale/rotation), NOT z_order.
        // No need to dirty the render cache - it only needs rebuilding when
        // entities are spawned/despawned or z_order changes.
      }
    }

    // --- Game Logic Update ---
    // Temporarily take ownership of the game to avoid borrow conflicts
    if let Some(mut game) = self.game.take() {
      // Track entity count before update to detect spawns/despawns
      let entity_count_before = self.world.len();

      // Create context for the game
      let mut ctx = GameContext {
        world: &mut self.world,
        camera: &mut self.camera,
        input: &self.input,
        assets: &mut self.assets,
        audio: &mut self.audio,
        effects: &mut self.effects,
        lighting: &mut self.lighting,
        events: &mut self.events,
        window_request: &mut self.window_request,
      };

      // Call user's update function
      game.update(&mut ctx, _dt);

      // Put the game back
      self.game = Some(game);

      // Only mark render cache dirty if entities were actually spawned/despawned
      if self.world.len() != entity_count_before {
        self.render_cache_dirty = true;
      }
    }

    // --- Hierarchy Transform Propagation ---
    // Update world transforms for all entities with parent-child relationships.
    // This must happen after user update (so local transforms are set) and
    // before rendering (so WorldTransform is current).
    self.world.propagate_transforms();

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

    // Check if world was cleared (generation changed) - this means physics must be reset
    // because hecs reuses entity IDs, so stale detection via contains() doesn't work
    let current_generation = self.world.generation();
    if current_generation != self.last_world_generation {
      self.physics.clear();
      self.last_world_generation = current_generation;
    }

    // Auto-unregister despawned entities from physics
    // (entities that were registered but no longer exist in the ECS world)
    // This handles normal gameplay despawning (not world.clear())
    let stale_physics_entities: Vec<Entity> = self
      .physics
      .registered_entities()
      .filter(|&e| !self.world.contains(e))
      .collect();

    for entity in stale_physics_entities {
      self.physics.unregister_entity(entity);
    }

    // Auto-register any new physics entities (RigidBody + Collider + Transform)
    let physics_entities: Vec<Entity> = self
      .world
      .entities_with::<RigidBody>()
      .into_iter()
      .filter(|&e| {
        !self.physics.is_registered(e)
          && self.world.get_component::<Transform>(e).is_some()
          && self.world.get_component::<Collider>(e).is_some()
      })
      .collect();

    for entity in physics_entities {
      let transform = self.world.get_component::<Transform>(entity).unwrap();
      let velocity = self.world.get_component::<Velocity>(entity);
      let rb = self.world.get_component::<RigidBody>(entity).unwrap();
      let col = self.world.get_component::<Collider>(entity).unwrap();
      self
        .physics
        .register_entity(entity, &transform, velocity.as_deref(), &rb, &col);
    }

    // Auto-register entities with GroupedCollider (for composite tiles)
    let grouped_entities: Vec<Entity> = self
      .world
      .entities_with::<RigidBody>()
      .into_iter()
      .filter(|&e| {
        !self.physics.is_registered(e)
          && self.world.get_component::<Transform>(e).is_some()
          && self.world.get_component::<GroupedCollider>(e).is_some()
      })
      .collect();

    for entity in grouped_entities {
      let transform = self.world.get_component::<Transform>(entity).unwrap();
      let velocity = self.world.get_component::<Velocity>(entity);
      let rb = self.world.get_component::<RigidBody>(entity).unwrap();
      let grouped = self.world.get_component::<GroupedCollider>(entity).unwrap();
      self
        .physics
        .register_grouped_entity(entity, &transform, velocity.as_deref(), &rb, &grouped);
    }

    // Sync ECS changes to Rapier before stepping physics
    // - Kinematic bodies: game code moves them directly via Transform
    // - Dynamic bodies: game code may teleport them (e.g., reset position)
    // - Velocities: game code may apply impulses via Velocity component
    self.physics.sync_kinematic_from_ecs(&self.world);
    self.physics.sync_dynamic_from_ecs(&self.world);
    self.physics.sync_velocities_from_ecs(&self.world);

    // Step the physics simulation
    let gravity = self.config.physics.gravity;
    self.physics.step(gravity);

    // Sync physics results back to ECS (Transform and Velocity)
    self.physics.sync_to_ecs(&mut self.world);
  }

  // =============================================================================
  // Render System - CPU & GPU Drawing
  // =============================================================================

  /// Render the current frame
  ///
  /// This is the "Render System" - it draws all entities with Transform + Renderable
  fn render(&mut self, _alpha: f32) {
    // Clear batch collector for this frame (used for GPU sprite batching)
    self.batch_collector.clear();

    // Get frame buffer from renderer
    let (frame, frame_width, frame_height) = match &mut self.renderer {
      Some(renderer) => {
        let w = renderer.game_width();
        let h = renderer.game_height();
        (renderer.frame_mut(), w, h)
      }
      None => return,
    };

    // Fill background with dark charcoal (game area only - letterbox is handled by clear color)
    for pixel in frame.chunks_exact_mut(4) {
      pixel[0] = 0x2d; // R: 45
      pixel[1] = 0x2d; // G: 45
      pixel[2] = 0x2d; // B: 45
      pixel[3] = 0xff; // A: 255
    }

    // --- Render Background Layers (Parallax, etc.) ---
    // Call game's render_background before entity rendering
    if let Some(mut game) = self.game.take() {
      game.render_background(frame, frame_width, frame_height, &self.camera);
      self.game = Some(game);
    }

    // --- Rebuild Render Cache (if dirty) ---
    // Rebuild render cache if dirty (entities added/removed or z_order changed)
    if self.render_cache_dirty {
      self.renderable_cache.clear();
      for entity in self.world.entities_with_two::<Transform, Renderable>() {
        if let Some(renderable) = self.world.get_component::<Renderable>(entity) {
          self.renderable_cache.push((entity, renderable.z_order));
        }
      }
      self.renderable_cache.sort_by_key(|(_, z)| *z);

      self.render_cache_dirty = false;
    }

    // --- Render All Entities (CPU Path) ---
    for &(entity, _) in &self.renderable_cache {
      let Some(renderable) = self.world.get_component::<Renderable>(entity) else {
        continue;
      };

      if !renderable.visible {
        continue;
      }

      // Use WorldTransform if available (for hierarchical entities),
      // otherwise fall back to local Transform
      let (x, y, rotation, scale_x, scale_y, pivot_x, pivot_y, offset_x, offset_y) =
        if let Some(wt) = self.world.get_component::<WorldTransform>(entity) {
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
            t.x, t.y, t.rotation, t.scale_x, t.scale_y, t.pivot_x, t.pivot_y, t.offset_x,
            t.offset_y,
          )
        } else {
          continue;
        };

      // Apply offset to base position (offset is a pixel-level adjustment after anchor)
      let x = x + offset_x;
      let y = y + offset_y;

      // draw_rotated expects degrees, Transform.rotation is in radians
      let rotation_deg = rotation.to_degrees();

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
              transformed_sprite.draw_rotated(
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
              );
            });
          } else {
            // No shader - draw normally
            renderable.sprite.with_sprite(|sprite| {
              sprite.draw_rotated(
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

            // Apply entity rotation to tile offset (need radians for trig)
            let rotation_rad = rotation_deg.to_radians();
            let cos_r = rotation_rad.cos();
            let sin_r = rotation_rad.sin();
            let rotated_x = local_x * cos_r - local_y * sin_r;
            let rotated_y = local_x * sin_r + local_y * cos_r;

            // Apply scale to rotated offset
            let scaled_x = rotated_x * scale_x;
            let scaled_y = rotated_y * scale_y;

            let world_x = x + scaled_x;
            let world_y = y + scaled_y;

            let (screen_x, screen_y) = self.camera.world_to_screen(world_x, world_y);
            let final_scale_x = scale_x * self.camera.zoom;
            let final_scale_y = scale_y * self.camera.zoom;

            renderable.sprite.with_sprite(|sprite| {
              sprite.draw_rotated(
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
    // If lighting is enabled and UI is not affected by lighting, render to separate buffer
    let render_ui_after_lighting = self
      .lighting
      .as_ref()
      .map(|l| l.is_enabled() && !l.is_ui_affected_by_lighting())
      .unwrap_or(false);

    if !render_ui_after_lighting {
      // Render UI directly to frame (will be affected by lighting if enabled)
      if let Some(mut game) = self.game.take() {
        let mut ui_ctx =
          UiContext::new(frame, frame_width, frame_height, &self.input, &self.ui_font);

        game.render_ui(&mut ui_ctx);

        self.game = Some(game);
      }
    } else {
      // Clear UI buffer and render UI to it (will be composited after lighting)
      self.ui_buffer.fill(0);
      if let Some(mut game) = self.game.take() {
        let mut ui_ctx = UiContext::new(
          &mut self.ui_buffer,
          frame_width,
          frame_height,
          &self.input,
          &self.ui_font,
        );

        game.render_ui(&mut ui_ctx);

        self.game = Some(game);
      }
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
      DisplayMode::Fullscreen => {
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

    self.window = Some(window);
    self.renderer = Some(renderer);
    self.sprite_gpu = Some(sprite_gpu);
    self.lighting = Some(lighting);
    self.trail_renderer = Some(trail_renderer);
    self.time = Some(TimeState::new(
      self.config.target_fps,
      self.config.physics_hz,
    ));

    // Call game init if not already initialized
    if !self.initialized {
      if let Some(mut game) = self.game.take() {
        let mut ctx = GameContext {
          world: &mut self.world,
          camera: &mut self.camera,
          input: &self.input,
          assets: &mut self.assets,
          audio: &mut self.audio,
          effects: &mut self.effects,
          lighting: &mut self.lighting,
          events: &mut self.events,
          window_request: &mut self.window_request,
        };
        game.init(&mut ctx);
        self.game = Some(game);
      }
      self.initialized = true;
      // Entities were spawned during init, rebuild render cache
      self.render_cache_dirty = true;
    }

    println!("Engine initialized successfully!");
    println!(
      "Game loop using fixed timestep: {}Hz updates",
      self.config.physics_hz
    );
  }

  fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
    // Handle input events
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
      _ => {}
    }

    match event {
      WindowEvent::CloseRequested => {
        println!("Close requested, exiting...");
        event_loop.exit();
      }
      WindowEvent::RedrawRequested => {
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

        // Fixed timestep update loop (includes game logic + physics)
        let mut did_update = false;
        let update_start = std::time::Instant::now();
        while self
          .time
          .as_ref()
          .map(|t| t.should_update())
          .unwrap_or(false)
        {
          let fixed_dt = self
            .time
            .as_ref()
            .expect("TimeState should be initialized")
            .fixed_dt();
          self.update(fixed_dt);
          if let Some(time) = &mut self.time {
            time.consume_update();
          }
          // Clear keyboard one-shot events after the FIRST update only.
          // This prevents key presses from being processed multiple times
          // when multiple fixed timestep updates run in a single frame,
          // while preserving mouse state for UI button clicks in render_ui.
          if !did_update {
            self.input.consume_keyboard_events();
            did_update = true;
          }
        }
        // Update time includes both game logic and physics
        self.frame_timings.update = update_start.elapsed();

        // Check if game wants to quit
        if self.game.as_ref().map(|g| g.should_quit()).unwrap_or(false) {
          event_loop.exit();
          return;
        }

        // Render with interpolation alpha
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
              && (self.config.debug.show_fps || self.config.debug.show_frame_timing)
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

        // Add GPU debug overlays AFTER game rendering (so they appear on top)
        // We use smoothed_timings for stable display (exponential moving average)
        if self.config.debug.show_frame_timing || self.config.debug.show_fps {
          let screen_w = self.config.width as f32;
          let screen_h = self.config.height as f32;

          if self.config.debug.show_frame_timing {
            debug::render_frame_timing_gpu(
              &mut self.batch_collector,
              &self.ui_font,
              &self.smoothed_timings,
              screen_w,
              screen_h,
            );
          } else if self.config.debug.show_fps {
            // Show FPS counter only if frame timing is not shown
            // (frame timing already includes FPS in its display)
            debug::render_fps_counter_gpu(
              &mut self.batch_collector,
              &self.ui_font,
              &self.smoothed_timings,
              screen_w,
              screen_h,
            );
          }
        }

        // Render time includes software rendering + GPU batch collection
        self.frame_timings.render = render_start.elapsed();

        // Actually draw to screen with post-processing effects, lighting, and GPU sprites
        // Take sprite_gpu, lighting, and trail_renderer out to avoid borrow conflicts with renderer
        let present_start = std::time::Instant::now();
        let mut sprite_gpu = self.sprite_gpu.take();
        let mut renderer = self.renderer.take();
        let mut lighting = self.lighting.take();
        let mut trail_renderer = self.trail_renderer.take();

        // Collect trails and lines from the world
        if let Some(ref mut trails) = trail_renderer {
          trails.begin_frame();

          // Add Trail2D components
          for entity in self.world.entities_with::<Trail2D>() {
            if let (Some(trail), Some(transform)) = (
              self.world.get_component::<Trail2D>(entity),
              self.world.get_component::<Transform>(entity),
            ) {
              trails.add_trail(&trail, &transform);
            }
          }

          // Add Line2D components
          for entity in self.world.entities_with::<Line2D>() {
            if let (Some(line), Some(transform)) = (
              self.world.get_component::<Line2D>(entity),
              self.world.get_component::<Transform>(entity),
            ) {
              trails.add_line(&line, transform.x, transform.y);
            }
          }
        }

        if let (Some(rend), Some(gpu)) = (&mut renderer, &mut sprite_gpu) {
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

          // Auto-enable lighting when Light2D entities exist
          if !lights.is_empty()
            && let Some(ref mut l) = lighting
            && !l.is_enabled()
          {
            l.set_enabled(true);
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
            .map(|t| t.frame_count as f32 / 60.0)
            .unwrap_or(0.0);

          // Check if lighting is enabled and should be applied
          let lighting_enabled = lighting
            .as_ref()
            .map(|l| l.is_enabled())
            .unwrap_or(false);

          let result = if has_gpu_sprites || has_trails || lighting_enabled {
            // Create callback closure that captures gpu, batch_collector, and trail_renderer
            let batch_collector = &self.batch_collector;
            let gpu_callback = |queue: &wgpu::Queue,
                                encoder: &mut wgpu::CommandEncoder,
                                view: &wgpu::TextureView| {
              // Render GPU sprites first
              if has_gpu_sprites {
                gpu.render_batches(queue, encoder, view, batch_collector);
              }
              // Render trails on top of sprites
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
              let lighting_callback =
                |device: &wgpu::Device,
                 queue: &wgpu::Queue,
                 encoder: &mut wgpu::CommandEncoder,
                 pixel_data: &[u8],
                 output_view: &wgpu::TextureView| {
                  if let Some(ref lighting_renderer) = lighting {
                    // Render lights to the light accumulation buffer
                    lighting_renderer.render_lights(
                      device,
                      queue,
                      encoder,
                      lights.iter().map(|(t, l)| (t, l)),
                      camera_pos,
                      camera_zoom,
                    );

                    // Compose the lit scene
                    lighting_renderer.compose(
                      device,
                      queue,
                      encoder,
                      pixel_data,
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
              rend.render_with_gpu_sprites(Some(&self.effects), Some(gpu_callback))
            }
          } else {
            rend.render_with_effects(Some(&self.effects))
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
              eprintln!("Render error: {:?}", e);
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

        // Calculate total frame time and FPS
        self.frame_timings.total = frame_start.elapsed();
        self.frame_timings.calculate_fps();

        // Debug: Log when a frame takes too long (>33ms = <30fps)
        #[cfg(debug_assertions)]
        {
          let total_ms = self.frame_timings.total.as_secs_f64() * 1000.0;
          if total_ms > 33.0 {
            eprintln!(
              "[SLOW FRAME] Total: {:.1}ms | Update: {:.1}ms | Render: {:.1}ms | Present: {:.1}ms",
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
            .update(&self.frame_timings, time.current_fps);

          let printed_debug = time.print_debug_info();

          // Print frame timing to console periodically (once per second) if enabled
          // This supplements the on-screen overlay for games using GPU sprites
          if self.config.debug.show_frame_timing && printed_debug {
            println!(
              "[FrameTiming] Update: {:.2}ms | Render: {:.2}ms | Present: {:.2}ms | Total: {:.2}ms",
              self.frame_timings.update.as_secs_f64() * 1000.0,
              self.frame_timings.render.as_secs_f64() * 1000.0,
              self.frame_timings.present.as_secs_f64() * 1000.0,
              self.frame_timings.total.as_secs_f64() * 1000.0,
            );
          }

          // Frame limiting: only apply manual limiting if presentation doesn't already block.
          // When vsync uses Fifo mode, the GPU blocks until vsync - adding sleep would
          // cause double-waiting and prevent hitting target framerate.
          // For Mailbox/Immediate modes, we use hybrid sleep/spin for precise timing.
          let presentation_blocks = self
            .renderer
            .as_ref()
            .map(|r| r.presentation_blocks())
            .unwrap_or(false);

          if !presentation_blocks {
            time.wait_for_target_frame_time();
          }
        }

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

  fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
    // Process any pending window configuration requests
    if self.window_request.has_changes()
      && let Some(window) = self.window.clone()
    {
      self.apply_window_request(&window);
    }

    if let Some(window) = &self.window {
      window.request_redraw();
    }
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
  /// Tracks whether display_mode was explicitly set (prevents build() from overwriting)
  display_mode_set: bool,
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
      display_mode_set: false,
    }
  }

  pub fn title(mut self, title: impl Into<String>) -> Self {
    self.config.title = title.into();
    self
  }

  pub fn resolution(mut self, width: u32, height: u32) -> Self {
    self.config.width = width;
    self.config.height = height;
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
  /// - [`DisplayMode::Fullscreen`] - Exclusive fullscreen (takes over display)
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

  /// Apply user preference overrides from system.json.
  ///
  /// Loads or creates the system state from the workspace's data directory
  /// using the proper fallback chain:
  ///
  /// 1. **system.json** (user settings) - highest priority
  /// 2. **Ryo.toml [system]** (developer defaults) - middle priority
  /// 3. **Engine defaults** - lowest priority (fallback)
  ///
  /// If system.json doesn't exist, it will be created from the developer
  /// defaults in Ryo.toml's [system] section combined with engine defaults.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_system_state_overrides()  // Apply user overrides from system.json
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_system_state_overrides(mut self) -> Self {
    if let Ok(workspace) = super::config::Workspace::discover()
      && let Ok(save_manager) = workspace.create_save_manager()
      && let Ok(mut system_state) = save_manager.load_or_create_system(&workspace.config().system)
    {
      // Migrate legacy fullscreen field if present
      system_state.display.migrate_legacy();

      // Apply display overrides
      self.config.width = system_state.display.width;
      self.config.height = system_state.display.height;
      self.config.display_mode = system_state.display.display_mode;
      self.display_mode_set = true;

      // Apply overrides if present
      if let Some(show_fps) = system_state.show_fps {
        self.config.debug.show_fps = show_fps;
      }
      // Store vsync preference to use when creating renderer
      self.vsync = system_state.vsync;
      // Store audio volumes to apply after AudioManager creation
      self.master_volume = system_state.master_volume;
      self.music_volume = system_state.music_volume;
      self.sfx_volume = system_state.sfx_volume;
    }
    self
  }

  /// Apply only display mode from system.json, keeping fixed resolution.
  ///
  /// This is useful for games with a fixed design resolution (e.g., pixel art games)
  /// that still want to respect the user's display mode preference (windowed/fullscreen).
  ///
  /// Unlike [`with_system_state_overrides`](Self::with_system_state_overrides), this method
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
    if let Ok(workspace) = super::config::Workspace::discover()
      && let Ok(save_manager) = workspace.create_save_manager()
      && let Ok(mut system_state) = save_manager.load_or_create_system(&workspace.config().system)
    {
      // Migrate legacy fullscreen field if present
      system_state.display.migrate_legacy();

      // Apply ONLY display mode - not resolution
      self.config.display_mode = system_state.display.display_mode;
      self.display_mode_set = true;

      // Apply overrides if present
      if let Some(show_fps) = system_state.show_fps {
        self.config.debug.show_fps = show_fps;
      }
      // Store vsync preference to use when creating renderer
      self.vsync = system_state.vsync;
      // Store audio volumes to apply after AudioManager creation
      self.master_volume = system_state.master_volume;
      self.music_volume = system_state.music_volume;
      self.sfx_volume = system_state.sfx_volume;
    }
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
    let display_mode_set = self.display_mode_set;

    // Priority chain for configuration:
    // 1. Explicit builder calls (highest) - already in self.config
    // 2. Embedded values from `ryo2d build` (compile-time)
    // 3. Runtime workspace discovery (Ryo.toml)
    // 4. Engine defaults (lowest) - already in EngineConfig::default()

    // Check for embedded values (set when built via `ryo2d build`)
    let has_embedded = option_env!("RYO_EMBED_RESOLUTION_WIDTH").is_some();

    if has_embedded {
      // Use compile-time embedded values from Ryo.toml
      if config.width == 800 && config.height == 600 {
        if let Some(w) = option_env!("RYO_EMBED_RESOLUTION_WIDTH") {
          config.width = w.parse().unwrap_or(800);
        }
        if let Some(h) = option_env!("RYO_EMBED_RESOLUTION_HEIGHT") {
          config.height = h.parse().unwrap_or(600);
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
      if let Some(vol) = option_env!("RYO_EMBED_MASTER_VOLUME") {
        master_volume = vol.parse().unwrap_or(1.0);
      }
      if let Some(vol) = option_env!("RYO_EMBED_MUSIC_VOLUME") {
        music_volume = vol.parse().unwrap_or(0.8);
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
      if !display_mode_set
        && let Some(mode) = option_env!("RYO_EMBED_DISPLAY_MODE")
      {
        config.display_mode = match mode.to_lowercase().as_str() {
          "fullscreen" => DisplayMode::Fullscreen,
          "borderless" | "borderless_fullscreen" => DisplayMode::BorderlessFullscreen,
          _ => DisplayMode::Windowed,
        };
      }
    } else if let Ok(workspace) = super::config::Workspace::discover() {
      // Fall back to runtime workspace discovery (development mode)
      let debug = &workspace.config().debug;
      let info = &workspace.config().info;
      let system = &workspace.config().system;

      // Load debug settings
      config.debug.show_colliders = debug.show_colliders;
      config.debug.show_fps = info.show_fps;
      config.debug.show_velocities = debug.show_velocities;
      config.debug.show_origins = debug.show_origins;
      config.debug.show_frame_timing = debug.show_frame_timing;

      // Load resolution from [system] section if not explicitly set via builder
      if config.width == 800 && config.height == 600 {
        config.width = system.resolution_width;
        config.height = system.resolution_height;
      }

      // Load display mode from [system] section (only if not explicitly set)
      if !display_mode_set {
        config.display_mode = system.display_mode;
      }
    }

    Engine::new(config, game, vsync, master_volume, music_volume, sfx_volume)
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
    assert_eq!(config.width, 800);
    assert_eq!(config.height, 600);
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
  fn time_state_initializes_correctly() {
    let time = TimeState::new(60, 120);
    let expected = 1.0 / 120.0;
    assert!((time.fixed_dt() - expected).abs() < 0.0001);
    assert!(!time.should_update()); // No time accumulated yet
  }

  #[test]
  fn time_state_accumulator_logic() {
    let mut time = TimeState::new(60, 60);

    // Simulate adding time
    time.accumulator = Duration::from_secs_f64(1.0 / 60.0);
    assert!(time.should_update());

    time.consume_update();
    assert!(!time.should_update());
  }

  #[test]
  fn time_state_caps_accumulator() {
    let mut time = TimeState::new(60, 60);

    // Simulate a huge frame spike
    time.delta_time = Duration::from_secs(1); // 1 second!
    time.accumulator = Duration::ZERO;

    time.begin_frame();

    // Should be capped to 5 * fixed_timestep
    let max = time.fixed_timestep * 5;
    assert!(time.accumulator <= max);
  }

  #[test]
  fn interpolation_alpha_is_bounded() {
    let mut time = TimeState::new(60, 60);

    time.accumulator = Duration::ZERO;
    assert_eq!(time.interpolation_alpha(), 0.0);

    time.accumulator = time.fixed_timestep;
    assert_eq!(time.interpolation_alpha(), 1.0);

    time.accumulator = time.fixed_timestep * 2; // Over 1.0
    assert_eq!(time.interpolation_alpha(), 1.0); // Clamped
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
}
