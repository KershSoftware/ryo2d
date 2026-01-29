use std::sync::Arc;
use std::time::{Duration, Instant};

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowAttributes, WindowId};

use super::asset::AssetManager;
use super::camera::Camera;
use super::ecs::{SpriteRenderer, Transform, Velocity, World};
use super::error::{EngineError, EngineResult};
use super::game::{Game, GameContext};
use super::input::InputState;
use super::physics::{Acceleration, PhysicsConfig, RigidBody};

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
      let fps = self.frame_count;
      println!(
        "[GameLoop] FPS: {}, Delta: {:.2}ms, Fixed DT: {:.2}ms",
        fps,
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
}

impl Default for EngineConfig {
  fn default() -> Self {
    EngineConfig {
      title: String::from("Ferrum2D"),
      width: 800,
      height: 600,
      target_fps: 60,
      physics_hz: 60,
      physics: PhysicsConfig::default(),
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
  // User-defined game logic (Option to allow taking ownership temporarily)
  game: Option<Box<dyn Game>>,
  // Whether init has been called
  initialized: bool,
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
    let assets = AssetManager::new();

    Engine {
      config,
      window: None,
      pixels: None,
      time: None,
      input: InputState::new(),
      world,
      camera,
      assets,
      game: Some(game),
      initialized: false,
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
  fn update(&mut self, dt: f32) {
    // Temporarily take ownership of the game to avoid borrow conflicts
    if let Some(mut game) = self.game.take() {
      // Create context for the game
      let mut ctx = GameContext {
        world: &mut self.world,
        camera: &mut self.camera,
        input: &self.input,
        assets: &mut self.assets,
      };

      // Call user's update function
      game.update(&mut ctx, dt);

      // Put the game back
      self.game = Some(game);
    }

    // === Physics System: Apply gravity and acceleration to velocity ===
    let gravity = self.config.physics.gravity;
    let entities = self.world.entities_with::<RigidBody>();
    for entity in entities {
      // Get physics properties
      let (gravity_scale, drag, is_kinematic) = {
        let rb = self
          .world
          .get_component::<RigidBody>(entity)
          .expect("Entity from entities_with::<RigidBody> should have RigidBody");
        (rb.gravity_scale, rb.drag, rb.is_kinematic)
      };

      // Skip kinematic bodies
      if is_kinematic {
        continue;
      }

      // Apply acceleration to velocity (if entity has both)
      if let Some(accel) = self.world.get_component::<Acceleration>(entity) {
        let (ax, ay) = (accel.ax, accel.ay);
        if let Some(mut vel) = self.world.get_component_mut::<Velocity>(entity) {
          vel.dx += ax * dt;
          vel.dy += ay * dt;
        }
      }

      // Apply gravity to velocity (scaled by gravity_scale)
      if let Some(mut vel) = self.world.get_component_mut::<Velocity>(entity) {
        vel.dx += gravity.0 * gravity_scale * dt;
        vel.dy += gravity.1 * gravity_scale * dt;

        // Apply drag (velocity decay)
        if drag > 0.0 {
          let decay = (1.0 - drag).powf(dt);
          vel.dx *= decay;
          vel.dy *= decay;
        }
      }
    }

    // === Movement System: Apply velocity to transform ===
    // This is an engine-level system that runs after physics
    let entities = self.world.entities_with_two::<Transform, Velocity>();
    for entity in entities {
      // Get velocity first (immutable borrow)
      let (dx, dy, dz) = {
        let velocity = self
          .world
          .get_component::<Velocity>(entity)
          .expect("Entity from entities_with_two::<Transform, Velocity> should have Velocity");
        (velocity.dx, velocity.dy, velocity.dz)
      };

      // Then modify transform (mutable borrow)
      if let Some(mut transform) = self.world.get_component_mut::<Transform>(entity) {
        transform.x += dx * dt;
        transform.y += dy * dt;
        transform.z += dz * dt;
      }
    }
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
    // Collect entities and sort by z_order (lower values drawn first, higher on top)
    let mut entities: Vec<_> = self
      .world
      .entities_with_two::<Transform, SpriteRenderer>()
      .into_iter()
      .filter_map(|entity| {
        let z = self.world.get_component::<SpriteRenderer>(entity)?.z_order;
        Some((entity, z))
      })
      .collect();
    entities.sort_by_key(|(_, z)| *z);

    for (entity, _) in entities {
      let transform = self
        .world
        .get_component::<Transform>(entity)
        .expect("Entity from entities_with_two::<Transform, SpriteRenderer> should have Transform");
      let sprite_renderer = self.world.get_component::<SpriteRenderer>(entity).expect(
        "Entity from entities_with_two::<Transform, SpriteRenderer> should have SpriteRenderer",
      );

      if sprite_renderer.visible {
        // Convert world position to screen position using camera
        let (screen_x, screen_y) = self.camera.world_to_screen(transform.x, transform.y);

        // Optionally cull off-screen sprites (optimization)
        let sprite_w = sprite_renderer.sprite.width() as f32;
        let sprite_h = sprite_renderer.sprite.height() as f32;
        if !self
          .camera
          .is_visible(transform.x, transform.y, sprite_w, sprite_h)
        {
          continue;
        }

        // Draw scaled by camera zoom
        sprite_renderer.sprite.draw_scaled(
          frame,
          frame_width,
          frame_height,
          screen_x as i32,
          screen_y as i32,
          self.camera.zoom,
        );
      }
    }
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
        };
        game.init(&mut ctx);
        self.game = Some(game);
      }
      self.initialized = true;
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

        // Clear per-frame input state AFTER update has read it
        self.input.begin_frame();

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

  /// Set the game implementation that will run on this engine.
  ///
  /// This is required - you must provide a game that implements the `Game` trait.
  pub fn with_game(mut self, game: impl Game + 'static) -> Self {
    self.game = Some(Box::new(game));
    self
  }

  pub fn build(self) -> Engine {
    let game = self
      .game
      .expect("EngineBuilder requires a game - call .with_game() before .build()");
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
    assert_eq!(config.title, "Ferrum2D");
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
