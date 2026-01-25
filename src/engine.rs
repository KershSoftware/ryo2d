use std::sync::Arc;
use std::time::{Duration, Instant};

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::PhysicalKey;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::input::InputState;
use crate::sprite::Sprite;

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
}

impl Default for EngineConfig {
  fn default() -> Self {
    EngineConfig {
      title: String::from("Ferrum2D"),
      width: 800,
      height: 600,
      target_fps: 60,
      physics_hz: 60,
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
  // Player state
  player_x: f32,
  player_y: f32,
  player_sprite: Option<Sprite>,
}

impl Engine {
  fn new(config: EngineConfig) -> Self {
    // Create a simple player sprite (32x32 white square with colored border)
    let player_sprite = Sprite::solid_rect(32, 32, 0x40, 0xc0, 0xff, 0xff);
    
    Engine {
      config,
      window: None,
      pixels: None,
      time: None,
      input: InputState::new(),
      player_x: 100.0,
      player_y: 100.0,
      player_sprite: Some(player_sprite),
    }
  }

  /// Run the engine
  pub fn run(mut self) -> Result<(), String> {
    println!("Initializing {}...", self.config.title);
    println!("  Resolution: {}x{}", self.config.width, self.config.height);
    println!("  Target FPS: {}", self.config.target_fps);

    let event_loop = EventLoop::new().map_err(|e| format!("Failed to create event loop: {}", e))?;

    event_loop
      .run_app(&mut self)
      .map_err(|e| format!("Event loop error: {}", e))
  }

  /// Fixed timestep update - called multiple times per frame if needed
  fn update(&mut self, dt: f32) {
    use winit::keyboard::KeyCode;
    
    // Player movement speed (pixels per second)
    let speed = 200.0;
    
    // Handle WASD / Arrow key movement
    if self.input.key_down(KeyCode::KeyW) || self.input.key_down(KeyCode::ArrowUp) {
      self.player_y -= speed * dt;
    }
    if self.input.key_down(KeyCode::KeyS) || self.input.key_down(KeyCode::ArrowDown) {
      self.player_y += speed * dt;
    }
    if self.input.key_down(KeyCode::KeyA) || self.input.key_down(KeyCode::ArrowLeft) {
      self.player_x -= speed * dt;
    }
    if self.input.key_down(KeyCode::KeyD) || self.input.key_down(KeyCode::ArrowRight) {
      self.player_x += speed * dt;
    }
    
    // Clamp player to screen bounds
    let max_x = (self.config.width - 32) as f32;
    let max_y = (self.config.height - 32) as f32;
    self.player_x = self.player_x.clamp(0.0, max_x);
    self.player_y = self.player_y.clamp(0.0, max_y);
  }

  /// Render the current frame
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
    
    // Draw the player sprite
    if let Some(sprite) = &self.player_sprite {
      sprite.draw(frame, frame_width, frame_height, self.player_x as i32, self.player_y as i32);
    }
  }
}

impl ApplicationHandler for Engine {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let window_attrs = WindowAttributes::default()
      .with_title(&self.config.title)
      .with_inner_size(winit::dpi::LogicalSize::new(self.config.width, self.config.height))
      .with_resizable(false);

    let window = Arc::new(
      event_loop
        .create_window(window_attrs)
        .expect("Failed to create window"),
    );

    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, Arc::clone(&window));

    let pixels = Pixels::new(self.config.width, self.config.height, surface_texture)
      .expect("Failed to create pixel buffer");

    self.window = Some(window);
    self.pixels = Some(pixels);
    self.time = Some(TimeState::new(self.config.target_fps, self.config.physics_hz));

    println!("Engine initialized successfully!");
    println!("Game loop using fixed timestep: {}Hz updates", self.config.physics_hz);
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
        // Begin frame - clear per-frame input state
        self.input.begin_frame();
        if let Some(time) = &mut self.time {
          time.begin_frame();
        }

        // Fixed timestep update loop
        while self.time.as_ref().map(|t| t.should_update()).unwrap_or(false) {
          let fixed_dt = self.time.as_ref().unwrap().fixed_dt();
          self.update(fixed_dt);
          if let Some(time) = &mut self.time {
            time.consume_update();
          }
        }

        // Render with interpolation alpha
        let alpha = self.time.as_ref().map(|t| t.interpolation_alpha()).unwrap_or(0.0);
        self.render(alpha);

        // Actually draw to screen
        if let Some(pixels) = &mut self.pixels {
          if let Err(e) = pixels.render() {
            eprintln!("Render error: {}", e);
            event_loop.exit();
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
}

impl EngineBuilder {
  pub fn new() -> Self {
    EngineBuilder {
      config: EngineConfig::default(),
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

  pub fn build(self) -> Engine {
    Engine::new(self.config)
  }
}

impl Default for EngineBuilder {
  fn default() -> Self {
    Self::new()
  }
}
