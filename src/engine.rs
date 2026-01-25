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
  running: bool,
}

impl Engine {
  fn new(config: EngineConfig) -> Self {
    Engine {
      config,
      running: false,
    }
  }

  /// Initialize the engine
  pub fn init(&mut self) -> Result<(), String> {
    println!("Initializing {}...", self.config.title);
    println!("  Resolution: {}x{}", self.config.width, self.config.height);
    println!("  Target FPS: {}", self.config.target_fps);

    Ok(())
  }

  /// Run the main game loop
  pub fn run(&mut self) -> Result<(), String> {
    println!("Starting game loop...");

    self.running = true;

    let mut frame_count = 0;
    while self.running && frame_count < 3 {
      println!("Frame {}", frame_count);
      frame_count += 1;

      if frame_count >= 3 {
        self.running = false;
      }
    }

    println!("Game loop ended.");

    Ok(())
  }

  /// Shutdown the engine and clean up resources
  pub fn shutdown(&mut self) {
    println!("Shutting down {}...", self.config.title);
    self.running = false;
  }

  /// Stop the game loop
  pub fn stop(&mut self) {
    self.running = false;
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
