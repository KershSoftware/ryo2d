use std::sync::Arc;
use std::time::{Duration, Instant};

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowAttributes, WindowId};

use super::animation::{AnimationController, Animator};
use super::asset::AssetManager;
use super::audio::AudioManager;
use super::camera::Camera;
use super::collision::Collider;
use super::debug::{self, DebugConfig};
use super::ecs::{Entity, SpriteRenderer, Transform, Velocity, World};
use super::error::{EngineError, EngineResult};
use super::game::{Game, GameContext};
use super::input::InputState;
use super::physics::{PhysicsConfig, RigidBody};
use super::rapier_physics::RapierPhysicsWorld;
use super::scene::{Scene, SceneGame};
use super::ui::{BitmapFont, UiContext, create_debug_font};

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

  fn print_debug_info(&mut self) {
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
    }
  }
}

/// The main engine struct
pub struct Engine {
  config: EngineConfig,
  window: Option<Arc<Window>>,
  pixels: Option<Pixels<'static>>,
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
  // User-defined game logic (Option to allow taking ownership temporarily)
  game: Option<Box<dyn Game>>,
  // Whether init has been called
  initialized: bool,
  // Cached sorted list of renderable entities (reused across frames)
  render_cache: Vec<(Entity, i32)>,
  // Whether render cache needs rebuilding (entities spawned/despawned, z_order changed)
  render_cache_dirty: bool,
  // Rapier2D physics world
  physics: RapierPhysicsWorld,
  // UI font for text rendering
  ui_font: BitmapFont,
  // Last known world generation (for detecting world.clear() calls)
  last_world_generation: u64,
}

impl Engine {
  fn new(config: EngineConfig, game: Box<dyn Game>) -> Self {
    let world = World::new();
    let camera = Camera::centered_at(
      config.width as f32 / 2.0,
      config.height as f32 / 2.0,
      config.width,
      config.height,
    );
    let mut assets = AssetManager::new();
    let ui_font = create_debug_font();

    // Enable hot reload in debug builds if a workspace with assets directory exists
    #[cfg(debug_assertions)]
    {
      if let Ok(workspace) = super::workspace::Workspace::discover() {
        let assets_dir = workspace.assets_dir();
        if assets_dir.exists()
          && let Err(e) = assets.enable_hot_reload(vec![assets_dir])
        {
          eprintln!("[HotReload] Failed to enable: {}", e);
        }
      }
    }

    // Try to initialize audio (gracefully handle no audio device)
    let audio = match AudioManager::new() {
      Ok(manager) => Some(manager),
      Err(e) => {
        eprintln!(
          "Warning: Could not initialize audio: {}. Continuing without sound.",
          e
        );
        None
      }
    };

    Engine {
      config,
      window: None,
      pixels: None,
      time: None,
      input: InputState::new(),
      world,
      camera,
      assets,
      audio,
      game: Some(game),
      initialized: false,
      render_cache: Vec::new(),
      render_cache_dirty: true,
      physics: RapierPhysicsWorld::new(),
      ui_font,
      last_world_generation: 0,
    }
  }

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

  /// Fixed timestep update - called multiple times per frame if needed
  ///
  /// This runs user game logic and built-in engine systems (physics, movement, camera).
  fn update(&mut self, _dt: f32) {
    // Process hot reloads (debug builds only)
    #[cfg(debug_assertions)]
    {
      let reloaded = self.assets.process_reloads();
      if reloaded > 0 {
        // Mark render cache dirty since sprites may have changed
        self.render_cache_dirty = true;
      }
    }

    // === Animation System ===
    // Update all Animator components
    for entity in self.world.entities_with::<Animator>() {
      if let Some(mut animator) = self.world.get_component_mut::<Animator>(entity) {
        animator.update(_dt);
      }
    }

    // Sync Animator sprites to SpriteRenderer
    let animator_entities: Vec<Entity> = self.world.entities_with::<Animator>();
    for entity in animator_entities {
      let sprite = self
        .world
        .get_component::<Animator>(entity)
        .and_then(|a| a.current_sprite().cloned());
      if let Some(sprite) = sprite
        && let Some(mut renderer) = self.world.get_component_mut::<SpriteRenderer>(entity)
      {
        renderer.sprite = sprite;
      }
    }

    // Update all AnimationController components
    for entity in self.world.entities_with::<AnimationController>() {
      if let Some(mut controller) = self.world.get_component_mut::<AnimationController>(entity) {
        controller.update(_dt);
      }
    }

    // Sync AnimationController sprites to SpriteRenderer
    let controller_entities: Vec<Entity> = self.world.entities_with::<AnimationController>();
    for entity in controller_entities {
      let sprite = self
        .world
        .get_component::<AnimationController>(entity)
        .and_then(|c| c.current_sprite().cloned());
      if let Some(sprite) = sprite
        && let Some(mut renderer) = self.world.get_component_mut::<SpriteRenderer>(entity)
      {
        renderer.sprite = sprite;
      }
    }

    // Temporarily take ownership of the game to avoid borrow conflicts
    if let Some(mut game) = self.game.take() {
      // Create context for the game with collision events from last physics step
      let mut ctx = GameContext {
        world: &mut self.world,
        camera: &mut self.camera,
        input: &self.input,
        assets: &mut self.assets,
        audio: &mut self.audio,
        collision_events: self.physics.get_collision_events(),
      };

      // Call user's update function
      game.update(&mut ctx, _dt);

      // Put the game back
      self.game = Some(game);

      // Mark render cache dirty in case entities were spawned/despawned
      self.render_cache_dirty = true;
    }

    // Check if physics should be paused
    let physics_paused = self
      .game
      .as_ref()
      .map(|g| g.is_physics_paused())
      .unwrap_or(false);

    // === Physics System (Rapier2D) ===
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

  /// Render the current frame
  ///
  /// This is the "Render System" - it draws all entities with Transform + SpriteRenderer
  fn render(&mut self, _alpha: f32) {
    // Get frame buffer
    let frame = match &mut self.pixels {
      Some(pixels) => pixels.frame_mut(),
      None => return,
    };

    let frame_width = self.config.width;
    let frame_height = self.config.height;

    // Fill background with dark blue
    for pixel in frame.chunks_exact_mut(4) {
      pixel[0] = 0x1e;
      pixel[1] = 0x40;
      pixel[2] = 0x5e;
      pixel[3] = 0xff;
    }

    // === Render System: Draw all entities with Transform + SpriteRenderer ===
    // Rebuild render cache if dirty (entities added/removed or z_order changed)
    if self.render_cache_dirty {
      self.render_cache.clear();
      for entity in self.world.entities_with_two::<Transform, SpriteRenderer>() {
        if let Some(renderer) = self.world.get_component::<SpriteRenderer>(entity) {
          self.render_cache.push((entity, renderer.z_order));
        }
      }
      self.render_cache.sort_by_key(|(_, z)| *z);
      self.render_cache_dirty = false;
    }

    for &(entity, _) in &self.render_cache {
      // Skip entities that no longer exist (stale cache entries)
      let Some(transform) = self.world.get_component::<Transform>(entity) else {
        continue;
      };
      let Some(sprite_renderer) = self.world.get_component::<SpriteRenderer>(entity) else {
        continue;
      };

      if sprite_renderer.visible {
        // Convert world position to screen position using camera
        let (screen_x, screen_y) = self.camera.world_to_screen(transform.x, transform.y);

        // Calculate final scale (transform scale * camera zoom)
        let final_scale_x = transform.scale_x * self.camera.zoom;
        let final_scale_y = transform.scale_y * self.camera.zoom;

        // Optionally cull off-screen sprites (optimization)
        let sprite_w = sprite_renderer.sprite.width() as f32 * final_scale_x;
        let sprite_h = sprite_renderer.sprite.height() as f32 * final_scale_y;
        if !self
          .camera
          .is_visible(transform.x, transform.y, sprite_w, sprite_h)
        {
          continue;
        }

        // Draw scaled by transform scale and camera zoom
        sprite_renderer.sprite.with_sprite(|sprite| {
          sprite.draw_scaled_xy(
            frame,
            frame_width,
            frame_height,
            screen_x as i32,
            screen_y as i32,
            final_scale_x,
            final_scale_y,
          );
        });
      }
    }

    // === Debug Rendering ===
    if self.config.debug.show_colliders {
      debug::render_colliders(frame, frame_width, frame_height, &self.world, &self.camera);
    }
    if self.config.debug.show_velocities {
      debug::render_velocities(frame, frame_width, frame_height, &self.world, &self.camera);
    }
    if self.config.debug.show_origins {
      debug::render_origins(frame, frame_width, frame_height, &self.world, &self.camera);
    }

    // === UI Rendering ===
    // Call game's render_ui with UiContext for immediate-mode UI
    if let Some(mut game) = self.game.take() {
      let mut ui_ctx = UiContext::new(frame, frame_width, frame_height, &self.input, &self.ui_font);

      game.render_ui(&mut ui_ctx);

      // Render FPS counter after game UI so it's always on top
      if self.config.debug.show_fps
        && let Some(time) = &self.time
      {
        debug::render_fps_counter(&mut ui_ctx, time.current_fps);
      }

      self.game = Some(game);
    }

    // === Apply screen overlay (for transitions) ===
    if let Some(game) = &self.game
      && let Some((alpha, color)) = game.get_overlay()
      && alpha > 0.001
    {
      blend_overlay(frame, color, alpha);
    }
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

impl ApplicationHandler for Engine {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let window_attrs = WindowAttributes::default()
      .with_title(&self.config.title)
      .with_inner_size(winit::dpi::LogicalSize::new(
        self.config.width,
        self.config.height,
      ))
      .with_resizable(false);

    let window = Arc::new(
      event_loop
        .create_window(window_attrs)
        .expect("Failed to create window"),
    );

    let window_size = window.inner_size();
    let surface_texture =
      SurfaceTexture::new(window_size.width, window_size.height, Arc::clone(&window));

    let pixels = Pixels::new(self.config.width, self.config.height, surface_texture)
      .expect("Failed to create pixel buffer");

    self.window = Some(window);
    self.pixels = Some(pixels);
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
          collision_events: &[], // No physics events during init
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
      }
      WindowEvent::MouseInput { state, button, .. } => {
        self.input.handle_mouse_button(*button, *state);
      }
      WindowEvent::CursorMoved { position, .. } => {
        self.input.handle_mouse_move(position.x, position.y);
      }
      WindowEvent::MouseWheel { delta, .. } => {
        self.input.handle_scroll(*delta);
      }
      _ => {}
    }

    match event {
      WindowEvent::CloseRequested => {
        println!("Close requested, exiting...");
        event_loop.exit();
      }
      WindowEvent::RedrawRequested => {
        if let Some(time) = &mut self.time {
          time.begin_frame();
        }

        // Fixed timestep update loop
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
        }

        // Check if game wants to quit
        if self.game.as_ref().map(|g| g.should_quit()).unwrap_or(false) {
          event_loop.exit();
          return;
        }

        // Render with interpolation alpha
        let alpha = self
          .time
          .as_ref()
          .map(|t| t.interpolation_alpha())
          .unwrap_or(0.0);
        self.render(alpha);

        // Actually draw to screen
        if let Some(pixels) = &mut self.pixels {
          match pixels.render() {
            Ok(_) => {}
            Err(e) => {
              eprintln!("Render error: {}", e);
              event_loop.exit();
            }
          }
        }

        // Debug output and frame limiting
        if let Some(time) = &mut self.time {
          time.print_debug_info();

          let frame_time = time.frame_start.elapsed();
          if frame_time < time.target_frame_time {
            std::thread::sleep(time.target_frame_time - frame_time);
          }
        }

        // Clear per-frame input state at END of frame (after update and render_ui have read it)
        self.input.begin_frame();
      }
      _ => {}
    }
  }

  fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
    if let Some(window) = &self.window {
      window.request_redraw();
    }
  }
}

/// Builder pattern for Engine configuration
pub struct EngineBuilder {
  config: EngineConfig,
  game: Option<Box<dyn Game>>,
}

impl EngineBuilder {
  pub fn new() -> Self {
    EngineBuilder {
      config: EngineConfig::default(),
      game: None,
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

  /// Load debug settings from the workspace's Ryo.toml file.
  ///
  /// This reads the `[debug]` section from the project's configuration
  /// file and applies it to the engine. Call this to automatically
  /// enable debug features configured in Ryo.toml.
  ///
  /// Note: User settings in system.json can override these values.
  /// Call `with_system_state_overrides()` after this to apply user preferences.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_workspace_debug()  // Reads [debug] from Ryo.toml
  ///     .with_system_state_overrides()  // Apply user overrides from system.json
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_workspace_debug(mut self) -> Self {
    if let Ok(workspace) = super::workspace::Workspace::discover() {
      let debug = &workspace.config().debug;
      self.config.debug.show_colliders = debug.show_colliders;
      self.config.debug.show_fps = debug.show_fps;
      self.config.debug.show_velocities = debug.show_velocities;
      self.config.debug.show_origins = debug.show_origins;
    }
    self
  }

  /// Apply user preference overrides from system.json.
  ///
  /// Loads the system state from the workspace's data directory and applies
  /// any overrides. This allows users to override engine settings without
  /// editing Ryo.toml.
  ///
  /// Note: system.json fields may not have a 1-to-1 mapping with Ryo.toml.
  /// For example, `show_fps` is a top-level field in system.json but a debug
  /// setting in Ryo.toml.
  ///
  /// # Example
  ///
  /// ```rust,ignore
  /// EngineBuilder::new()
  ///     .title("My Game")
  ///     .with_workspace_debug()  // Load Ryo.toml defaults
  ///     .with_system_state_overrides()  // Apply user overrides from system.json
  ///     .with_scene(GameScene::new())
  ///     .build()
  ///     .run()?;
  /// ```
  pub fn with_system_state_overrides(mut self) -> Self {
    if let Ok(workspace) = super::workspace::Workspace::discover() {
      if let Ok(save_manager) = workspace.create_save_manager() {
        if let Ok(system_state) = save_manager.load_system() {
          // Apply overrides if present
          if let Some(show_fps) = system_state.show_fps {
            self.config.debug.show_fps = show_fps;
          }
        }
      }
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
    Engine::new(self.config, game)
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
}
