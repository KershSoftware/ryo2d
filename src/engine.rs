use std::sync::Arc;

use pixels::{Pixels, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

/// Configuration for the engine
#[derive(Debug, Clone)]
pub struct EngineConfig {
  pub title: String,
  pub width: u32,
  pub height: u32,
  pub target_fps: u32,
}

impl Default for EngineConfig {
  fn default() -> Self {
    EngineConfig {
      title: String::from("Ferrum2D"),
      width: 800,
      height: 600,
      target_fps: 60,
    }
  }
}

/// The main engine struct
pub struct Engine {
  config: EngineConfig,
  window: Option<Arc<Window>>,
  pixels: Option<Pixels<'static>>,
}

impl Engine {
  fn new(config: EngineConfig) -> Self {
    Engine {
      config,
      window: None,
      pixels: None,
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

    println!("Engine initialized successfully!");
  }

  fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
    match event {
      WindowEvent::CloseRequested => {
        println!("Close requested, exiting...");
        event_loop.exit();
      }
      WindowEvent::RedrawRequested => {
        if let Some(pixels) = &mut self.pixels {
          let frame = pixels.frame_mut();
          for pixel in frame.chunks_exact_mut(4) {
            pixel[0] = 0x1e;
            pixel[1] = 0x40;
            pixel[2] = 0x5e;
            pixel[3] = 0xff;
          }

          if let Err(e) = pixels.render() {
            eprintln!("Render error: {}", e);
            event_loop.exit();
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

  pub fn build(self) -> Engine {
    Engine::new(self.config)
  }
}

impl Default for EngineBuilder {
  fn default() -> Self {
    Self::new()
  }
}
